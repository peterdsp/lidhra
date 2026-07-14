/**
 * Lidhra license service (Cloudflare Worker).
 *
 * Two jobs, both automatic:
 *   POST /kofi      Ko-fi Shop-Order webhook. Verifies the token, records the
 *                   buyer's email as "purchased" in KV, and emails them a short
 *                   "open the app and activate" note. No key is ever emailed, so
 *                   nothing shareable leaks.
 *   POST /activate  Called by the Lidhra app. Body: { email, machine_id }.
 *                   If that email purchased and is under the activation cap, it
 *                   mints a node-locked Ed25519 key (subject MACHINE:<id>) that
 *                   only works on that machine, and returns { key }.
 *
 * The key format matches crates/lidhra-license exactly:
 *   LIDHRA-<base64url(subject)>.<base64url(ed25519_sig(subject))>
 *
 * Secrets (wrangler secret put):  ISSUER_SEED_HEX, KOFI_TOKEN, RESEND_API_KEY
 * Vars (wrangler.toml):           KOFI_PRODUCT_CODE, FROM_EMAIL, MAX_ACTIVATIONS
 * KV binding:                     LICENSES
 */
import * as ed from "@noble/ed25519";

// noble-ed25519 v2 needs a sha512; use the platform WebCrypto in the Worker.
ed.etc.sha512Async = async (...m) =>
  new Uint8Array(await crypto.subtle.digest("SHA-512", ed.etc.concatBytes(...m)));

const enc = new TextEncoder();

function b64url(bytes) {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function hexTo32(hex) {
  hex = hex.trim();
  if (hex.length !== 64) throw new Error("ISSUER_SEED_HEX must be 64 hex chars");
  const out = new Uint8Array(32);
  for (let i = 0; i < 32; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
  return out;
}

/** Mint a key for `subject`, signed with the issuer seed. Mirrors lidhra-keygen. */
async function mintKey(subject, seedHex) {
  const seed = hexTo32(seedHex);
  const msg = enc.encode(subject);
  const sig = await ed.signAsync(msg, seed);
  return `LIDHRA-${b64url(msg)}.${b64url(sig)}`;
}

const json = (obj, status = 200, extra = {}) =>
  new Response(JSON.stringify(obj), {
    status,
    headers: { "content-type": "application/json", ...extra },
  });

async function sendEmail(env, to, fromName) {
  if (!env.RESEND_API_KEY) return; // email is optional; activation still works
  const html = `
    <div style="font-family:-apple-system,Segoe UI,Roboto,Arial,sans-serif;color:#0c1a16">
      <h2 style="margin:0 0 8px">Thanks for buying Lidhra${fromName ? ", " + fromName : ""}!</h2>
      <p>Your licence is ready. To activate:</p>
      <ol>
        <li>Open Lidhra (download it from
          <a href="https://github.com/peterdsp/lidhra/releases/latest">the releases page</a>
          if you have not yet).</li>
        <li>When the trial banner appears, choose <b>Activate with your Ko-fi email</b>.</li>
        <li>Enter <b>this email address</b>. That is it, the app licenses itself.</li>
      </ol>
      <p style="color:#46584f">Your licence covers up to ${env.MAX_ACTIVATIONS || 3} devices.
      Questions? Just reply to your Ko-fi message.</p>
    </div>`;
  await fetch("https://api.resend.com/emails", {
    method: "POST",
    headers: {
      authorization: `Bearer ${env.RESEND_API_KEY}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      from: `Lidhra <${env.FROM_EMAIL || "licenses@lidhra.peterdsp.dev"}>`,
      to: [to],
      subject: "Your Lidhra licence",
      html,
    }),
  }).catch(() => {});
}

// ---- Ko-fi Shop-Order webhook -------------------------------------------
async function handleKofi(req, env) {
  const form = await req.formData();
  let data;
  try {
    data = JSON.parse(form.get("data"));
  } catch {
    return json({ error: "bad payload" }, 400);
  }
  if (data.verification_token !== env.KOFI_TOKEN) {
    return json({ error: "bad token" }, 401);
  }
  // Only act on a purchase of the Lidhra product (if a code is configured).
  const code = (env.KOFI_PRODUCT_CODE || "").trim();
  const items = data.shop_items || [];
  const matches =
    !code || items.some((it) => (it.direct_link_code || "") === code);
  if (data.type !== "Shop Order" || !matches) {
    return json({ ok: true, skipped: true }); // ack so Ko-fi stops retrying
  }
  const email = (data.email || "").trim().toLowerCase();
  if (!email) return json({ error: "no email" }, 400);

  const rec = {
    purchased_at: data.timestamp || null,
    order_id: data.kofi_transaction_id || data.message_id || null,
    name: data.from_name || null,
  };
  await env.LICENSES.put(`buyer:${email}`, JSON.stringify(rec));
  await sendEmail(env, email, data.from_name);
  return json({ ok: true });
}

// ---- In-app online activation -------------------------------------------
async function handleActivate(req, env) {
  let body;
  try {
    body = await req.json();
  } catch {
    return json({ error: "bad json" }, 400);
  }
  const email = (body.email || "").trim().toLowerCase();
  const machine = (body.machine_id || "").trim();
  if (!email) return json({ error: "email required" }, 400);

  const buyer = await env.LICENSES.get(`buyer:${email}`);
  if (!buyer) {
    return json(
      { error: "No purchase found for that email. Use the exact address you paid with on Ko-fi." },
      403
    );
  }

  const cap = parseInt(env.MAX_ACTIVATIONS || "3", 10);
  const key = `machines:${email}`;
  const seen = JSON.parse((await env.LICENSES.get(key)) || "[]");
  if (machine && !seen.includes(machine)) {
    if (seen.length >= cap) {
      return json({ error: `Activation limit reached (${cap} devices).` }, 403);
    }
    seen.push(machine);
    await env.LICENSES.put(key, JSON.stringify(seen));
  }

  // Node-lock to the machine when we have an id; otherwise bind to the email.
  const subject = machine ? `MACHINE:${machine}` : email;
  const licence = await mintKey(subject, env.ISSUER_SEED_HEX);
  return json({ key: licence, subject, devices: seen.length || 1 });
}

export default {
  async fetch(req, env) {
    const url = new URL(req.url);
    try {
      if (req.method === "POST" && url.pathname === "/kofi") return await handleKofi(req, env);
      if (req.method === "POST" && url.pathname === "/activate") return await handleActivate(req, env);
      if (url.pathname === "/" || url.pathname === "/health")
        return json({ service: "lidhra-license", ok: true });
      return json({ error: "not found" }, 404);
    } catch (e) {
      return json({ error: String(e && e.message || e) }, 500);
    }
  },
};
