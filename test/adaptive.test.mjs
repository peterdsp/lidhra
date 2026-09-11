// Unit tests for the shared adaptive-layout math in ui/index.html.
//
// The pure logic lives inside a marked block (ADAPTIVE:START..ADAPTIVE:END) in
// the single shipped HTML file, so there is nothing to bundle or import at
// runtime. This test extracts that exact block and runs it in a sandbox, which
// guarantees the code under test is byte-for-byte the code that ships.
//
//   node --test test/adaptive.test.mjs
//
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import vm from 'node:vm';

const here = dirname(fileURLToPath(import.meta.url));
const html = readFileSync(join(here, '..', 'ui', 'index.html'), 'utf8');

const start = html.indexOf('/* ADAPTIVE:START');
const end = html.indexOf('/* ADAPTIVE:END */');
assert.ok(start >= 0 && end > start, 'ADAPTIVE block not found in ui/index.html');
const block = html.slice(start, end);

const sandbox = {};
vm.createContext(sandbox);
vm.runInContext(block, sandbox, { filename: 'adaptive-block.js' });
const A = sandbox.LidhraAdaptive;
assert.ok(A && typeof A.chooseLayout === 'function', 'LidhraAdaptive did not initialise');

const T = A.THRESHOLDS;
// The module runs in a separate realm, so its objects have a different
// prototype and strict deepEqual would reject them; compare by value.
const same = (got, exp) => assert.equal(JSON.stringify(got), JSON.stringify(exp));

test('coordinate conversion honours a measured scale and origin', () => {
  // points -> css px at scale 2, webview offset (40,10) css px.
  const r = A.toCssRect({ x: 100, y: 50, w: 20, h: 200 }, { scale: 2, originX: 40, originY: 10 });
  same(r, { x: 160, y: 90, w: 40, h: 400 });
  // no transform is identity, never assumes 1:1 silently
  same(A.toCssRect({ x: 5, y: 6, w: 7, h: 8 }, {}), { x: 5, y: 6, w: 7, h: 8 });
  // a zero/negative scale is rejected in favour of 1 rather than collapsing
  assert.equal(A.toCssRect({ x: 10, y: 0, w: 2, h: 2 }, { scale: 0 }).x, 10);
});

test('phone width with no regions is a single compact pane', () => {
  const r = A.chooseLayout({ width: 390, height: 844, sidebar: 0, regions: [] });
  assert.equal(r.mode, 'compact');
  assert.equal(r.divide, null);
});

test('a narrow window stays compact even above nothing', () => {
  assert.equal(A.chooseLayout({ width: 700, height: 900, sidebar: 0, regions: [] }).mode, 'compact');
});

test('a tablet with the sidebar but no room for two panes is regular', () => {
  const r = A.chooseLayout({ width: 834, height: 1112, sidebar: T.SIDEBAR, regions: [] });
  assert.equal(r.mode, 'regular');
});

test('a wide window splits into list + detail', () => {
  const r = A.chooseLayout({ width: 1280, height: 800, sidebar: T.SIDEBAR, regions: [] });
  assert.equal(r.mode, 'split');
  assert.equal(r.divide, null);
});

test('exactly at the split threshold it splits, one pixel below it does not', () => {
  const need = T.SIDEBAR + T.LIST_MIN + T.DETAIL_MIN + T.GAP;
  assert.equal(A.chooseLayout({ width: need, height: 800, sidebar: T.SIDEBAR, regions: [] }).mode, 'split');
  assert.equal(A.chooseLayout({ width: need - 1, height: 800, sidebar: T.SIDEBAR, regions: [] }).mode, 'regular');
});

test('a central vertical fold defines two side-by-side regions', () => {
  const r = A.chooseLayout({ width: 1000, height: 760, sidebar: T.SIDEBAR,
    regions: [{ x: 489, y: 0, w: 22, h: 760, kind: 'hinge' }] });
  assert.equal(r.mode, 'split');
  assert.ok(r.divide && r.divide.axis === 'v');
  assert.equal(r.divide.start, 489);
  assert.equal(r.divide.size, 22);
});

test('a fold drops the sidebar before squeezing the work areas', () => {
  // left region 489 minus a 280 sidebar (209) is below LIST_MIN, but 489 alone
  // clears it, so the sidebar is collapsed rather than giving up on two panes.
  const r = A.chooseLayout({ width: 1000, height: 760, sidebar: T.SIDEBAR,
    regions: [{ x: 489, y: 0, w: 22, h: 760 }] });
  assert.equal(r.mode, 'split');
  assert.equal(r.sidebar, 0);
});

test('a fold too narrow for two usable panes falls back to a single pane', () => {
  // fold near the left edge: left region ~120 < LIST_MIN, so it is occluded and
  // the layout becomes a complete single pane clear of the fold.
  const r = A.chooseLayout({ width: 900, height: 760, sidebar: 0,
    regions: [{ x: 120, y: 0, w: 20, h: 760 }] });
  assert.notEqual(r.mode, 'split');
  assert.equal(r.divide, null);
  assert.ok(r.insets.left >= 140, 'smaller side is occluded');
});

test('a central horizontal fold stacks the work areas when each half is tall', () => {
  const r = A.chooseLayout({ width: 900, height: 1000, sidebar: 0,
    regions: [{ x: 0, y: 492, w: 900, h: 16 }] });
  assert.equal(r.mode, 'stack');
  assert.ok(r.divide && r.divide.axis === 'h');
});

test('a horizontal fold with too little height per half does not stack', () => {
  const r = A.chooseLayout({ width: 900, height: 300, sidebar: 0,
    regions: [{ x: 0, y: 150, w: 900, h: 10 }] });
  assert.notEqual(r.mode, 'stack');
});

test('an edge-touching region is an occlusion inset, not a fold', () => {
  const a = A.analyzeRegions([{ x: 0, y: 0, w: 24, h: 400 }], 1000, 800);
  assert.equal(a.insets.left, 24);
  assert.equal(a.vDivide, null);
  assert.equal(a.hDivide, null);
});

test('a free-floating region touching no edge opens no gap', () => {
  const a = A.analyzeRegions([{ x: 300, y: 300, w: 40, h: 40 }], 1000, 800);
  same(a.insets, { top: 0, right: 0, bottom: 0, left: 0 });
  assert.equal(a.vDivide, null);
  assert.equal(a.hDivide, null);
});

test('empty and zero-area regions contribute nothing', () => {
  same(A.analyzeRegions([], 1000, 800).insets, { top: 0, right: 0, bottom: 0, left: 0 });
  same(A.analyzeRegions([{ x: 10, y: 10, w: 0, h: 500 }], 1000, 800).insets,
    { top: 0, right: 0, bottom: 0, left: 0 });
});

test('geometry revisions: only a newer, well-formed, v1 payload is accepted', () => {
  assert.equal(A.acceptRevision({ v: 1, rev: 5 }, 2), 5);   // newer -> accept
  assert.equal(A.acceptRevision({ v: 1, rev: 2 }, 5), -1);  // stale -> drop
  assert.equal(A.acceptRevision({ v: 1, rev: 5 }, 5), -1);  // replay -> drop
  assert.equal(A.acceptRevision({ v: 2, rev: 9 }, 2), -1);  // wrong version -> drop
  assert.equal(A.acceptRevision({ rev: 9 }, 2), -1);        // no version -> drop
  assert.equal(A.acceptRevision(null, 2), -1);              // no payload -> drop
  assert.equal(A.acceptRevision({ v: 1, rev: 'x' }, 2), -1);// bad rev -> drop
});
