#!/usr/bin/env python3
# Generates native-vector SVG artboards of the Lidhra app UI, one per platform.
import os
OUT = os.path.dirname(os.path.abspath(__file__))

# ---- palette ----
PAPER="#f2f8f5"; CARD="#ffffff"; LINE="#d8e4dd"; INK="#0a1512"
FG="#0c1a16"; FGS="#46584f"; FGM="#7a8a82"
B1="#15c3b6"; B2="#2fd191"; B3="#54e06a"
DOWN="#22bd7a"; SEED="#11a594"; UP="#17b6c2"; PAUSE="#8a94a8"
SANS="Helvetica Neue, Helvetica, Arial, sans-serif"
MONO="Menlo, Consolas, monospace"

W,H = 1400, 940

def esc(s): return s.replace("&","&amp;").replace("<","&lt;").replace(">","&gt;")

def txt(x,y,s,size=14,fill=FG,w=400,family=SANS,anchor="start",ls=None,op=None):
    a=f' text-anchor="{anchor}"' if anchor!="start" else ""
    l=f' letter-spacing="{ls}"' if ls else ""
    o=f' opacity="{op}"' if op else ""
    return f'<text x="{x}" y="{y}" font-family="{family}" font-size="{size}" font-weight="{w}" fill="{fill}"{a}{l}{o}>{esc(s)}</text>'

def rrect(x,y,w,h,r,fill="none",stroke=None,sw=1,op=None,extra=""):
    st=f' stroke="{stroke}" stroke-width="{sw}"' if stroke else ""
    o=f' opacity="{op}"' if op else ""
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}"{" " if extra else ""}{extra} fill="{fill}"{st}{o}/>'

def bar(x,y,w,pct,color,track="#c9d8d0",h=6):
    fw=max(0,min(w,w*pct/100))
    return (f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{h/2}" fill="{track}"/>'
            f'<rect x="{x}" y="{y}" width="{fw:.1f}" height="{h}" rx="{h/2}" fill="{color}"/>')

def mark(x,y,s,stroke="url(#lg)"):
    sc=s/1024.0
    return (f'<g transform="translate({x},{y}) scale({sc})" fill="none" stroke="{stroke}" '
            f'stroke-width="112" stroke-linecap="round" stroke-linejoin="round">'
            f'<path d="M 348 258 C 214 315, 210 510, 326 594 C 414 658, 490 618, 550 558"/>'
            f'<path d="M 676 766 C 810 709, 814 514, 698 430 C 610 366, 534 406, 474 466"/></g>')

def defs():
    return ('<defs>'
      f'<linearGradient id="lg" x1="0" y1="0" x2="1" y2="1">'
      f'<stop offset="0" stop-color="{B1}"/><stop offset=".55" stop-color="{B2}"/>'
      f'<stop offset="1" stop-color="{B3}"/></linearGradient>'
      f'<linearGradient id="lgh" x1="0" y1="0" x2="1" y2="0">'
      f'<stop offset="0" stop-color="{B1}"/><stop offset=".55" stop-color="{B2}"/>'
      f'<stop offset="1" stop-color="{B3}"/></linearGradient>'
      f'<radialGradient id="glow" cx="82%" cy="2%" r="55%">'
      f'<stop offset="0" stop-color="#2fd191" stop-opacity=".18"/>'
      f'<stop offset="1" stop-color="#2fd191" stop-opacity="0"/></radialGradient>'
      '</defs>')

def artboard(platform, dstag, page, body, caption):
    s=[f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">']
    s.append(defs())
    s.append(rrect(0,0,W,H,0,fill=PAPER))
    s.append(rrect(0,0,W,H,0,fill="url(#glow)"))
    # header
    s.append(mark(54,40,30))
    s.append(txt(96,62,"LIDHRA",21,INK,800,ls="2"))
    s.append(txt(212,62,"/ app designs",14,FGM,500,MONO))
    s.append(txt(W-54,50,platform,26,FG,800,anchor="end"))
    s.append(txt(W-54,74,dstag,13,FGM,500,MONO,anchor="end"))
    s.append(f'<rect x="54" y="92" width="{W-108}" height="4" rx="2" fill="url(#lgh)"/>')
    s.append(f'<g>{body}</g>')
    # footer
    s.append(f'<rect x="54" y="{H-52}" width="{W-108}" height="1" fill="{LINE}"/>')
    s.append(txt(54,H-28,caption,13,FGM,400,MONO))
    s.append(txt(W-54,H-28,page,12,FGM,500,MONO,anchor="end"))
    s.append('</svg>')
    return "\n".join(s)

# ---------- reusable content pieces ----------
def torrent_row_full(x,y,w,name,meta,pct,color,spd,spd_col,eta):
    g=[txt(x,y, name,13.5,FG,600)]
    g.append(txt(x,y+17, meta,11,FGM,400,MONO))
    g.append(bar(x,y+25,w-170,pct,color))
    g.append(txt(x+w,y+6, spd,12.5,spd_col,600,MONO,anchor="end"))
    g.append(txt(x+w,y+24, eta,11,FGM,400,MONO,anchor="end"))
    return "".join(g)

# ============================================================
# 1) macOS  — menu-bar glass panel + main window
# ============================================================
def macos():
    b=[]
    # ---- menu-bar panel (left) ----
    px,py,pw = 90,150,360
    # menu bar strip
    b.append(rrect(px,py,pw,26,8,fill="#eef4f0",stroke=LINE,sw=1))
    b.append(txt(px+pw-16,py+18,"9:41",12,FGS,500,MONO,anchor="end"))
    b.append(mark(px+pw-150,py+5,16))
    b.append(txt(px+pw-58,py+18,"100%",11,FGM,500,MONO,anchor="end"))
    # glass panel
    gy=py+34; gh=290
    b.append(rrect(px,gy,pw,gh,18,fill="#ffffff",stroke="#e6efe9",sw=1))
    b.append(rrect(px,gy,pw,gh,18,fill="url(#glow)"))
    b.append(mark(px+16,gy+14,18)); b.append(txt(px+42,gy+28,"Lidhra",15,FG,700))
    b.append(rrect(px+pw-138,gy+12,122,22,11,fill="#dbf3e8"))
    b.append(txt(px+pw-77,gy+27,"↓ 12.4 · ↑ 2.1",11,"#0b8f79",600,MONO,anchor="middle"))
    rows=[("ubuntu-24.04.2-desktop-amd64.iso","62% · 14 min left",62,DOWN,"12.4 MB/s",DOWN),
          ("debian-13-netinst.iso","Seeding · ratio 3.4",100,SEED,"↑ 2.1 MB/s",SEED),
          ("archlinux-2026.07.01-x86_64.iso","Paused · 88%",88,PAUSE,"—",FGM)]
    ry=gy+52
    for nm,st,pct,col,spd,sc in rows:
        b.append(rrect(px+10,ry,pw-20,58,10,fill="#f6faf8"))
        b.append(rrect(px+20,ry+13,30,30,8,fill=col if col!=PAUSE else "#aeb8b2"))
        b.append(txt(px+58,ry+22,nm[:30],12.5,FG,600))
        b.append(txt(px+58,ry+38,st,10.5,FGM,400,MONO))
        b.append(txt(px+pw-20,ry+22,spd,11,sc,600,MONO,anchor="end"))
        b.append(bar(px+58,ry+44,pw-88,pct,col))
        ry+=66
    b.append(f'<rect x="{px+14}" y="{ry+2}" width="{pw-28}" height="1" fill="{LINE}"/>')
    b.append(txt(px+18,ry+22,"3 active · 1 paused",12,FGS,400))
    b.append(rrect(px+pw-86,ry+8,70,24,12,fill="url(#lgh)"))
    b.append(txt(px+pw-51,ry+24,"+ Add",12,"#06251b",700,anchor="middle"))
    b.append(txt(px,gy+gh+34,"Menu-bar glance — Liquid Glass panel hangs from the bar.",12.5,FGS,400))

    # ---- main window (right) ----
    wx,wy,ww,wh = 540,150,760,470
    b.append(rrect(wx,wy,ww,wh,14,fill=CARD,stroke=LINE,sw=1))
    b.append(rrect(wx,wy,ww,40,14,fill="#f6faf8"))
    b.append(rrect(wx,wy+26,ww,14,0,fill=CARD))  # square off bottom of titlebar
    b.append(rrect(wx,wy,ww,wh,14,fill="none",stroke=LINE,sw=1))
    for i,c in enumerate(["#ff5f57","#febc2e","#28c840"]):
        b.append(f'<circle cx="{wx+22+i*20}" cy="{wy+20}" r="6.5" fill="{c}"/>')
    b.append(mark(wx+86,wy+8,16)); b.append(txt(wx+112,wy+25,"Lidhra",13,FG,700))
    b.append(txt(wx+ww-18,wy+25,"⌘F  Filter…",11,FGM,400,MONO,anchor="end"))
    b.append(f'<line x1="{wx}" y1="{wy+40}" x2="{wx}" y2="{wy+wh}" stroke="none"/>')
    # sidebar
    sbx=wx; sbw=190
    b.append(f'<rect x="{sbx}" y="{wy+40}" width="{sbw}" height="{wh-40}" fill="#f9fcfa"/>')
    b.append(f'<line x1="{sbx+sbw}" y1="{wy+40}" x2="{sbx+sbw}" y2="{wy+wh}" stroke="{LINE}"/>')
    side=[("LIBRARY",None,None,True),("Overview","12",True,False),("Transfers","3",False,False),
          ("Seeding","7",False,False),("Paused","2",False,False),
          ("CATEGORIES",None,None,True),("Linux","8",False,False),("Media","4",False,False)]
    sy=wy+66
    for label,cnt,sel,grp in side:
        if grp:
            b.append(txt(sbx+20,sy,label,10,FGM,700,MONO,ls="1.4")); sy+=24; continue
        if sel:
            b.append(rrect(sbx+12,sy-15,sbw-24,26,7,fill="#dcf1e7"))
        b.append(txt(sbx+22,sy+2,("◆ " if not grp else "")+label,12.5,"#0b8f79" if sel else FGS,600 if sel else 400))
        if cnt: b.append(txt(sbx+sbw-20,sy+2,cnt,10.5,FGM,400,MONO,anchor="end"))
        sy+=30
    # main list
    mx=sbx+sbw+22; mw=ww-sbw-44
    b.append(rrect(mx,wy+58,64,26,8,fill="url(#lgh)")); b.append(txt(mx+32,wy+75,"+ Add",12,"#06251b",700,anchor="middle"))
    b.append(rrect(mx+72,wy+58,74,26,8,fill="#eef4f0",stroke=LINE,sw=1)); b.append(txt(mx+109,wy+75,"Pause all",11,FGS,400,anchor="middle"))
    b.append(txt(wx+ww-30,wy+75,"↓ 12.4 · ↑ 2.1 MB/s",11,FGM,500,MONO,anchor="end"))
    rows=[("ubuntu-24.04.2-desktop-amd64.iso","4.7 GB · 62% · 214 peers",62,DOWN,"12.4 MB/s",DOWN,"14m"),
          ("debian-13-netinst.iso","631 MB · seeding · ratio 3.4",100,SEED,"↑ 2.1",SEED,"∞"),
          ("archlinux-2026.07.01-x86_64.iso","1.1 GB · paused · 88%",88,PAUSE,"—",FGM,"—")]
    ry=wy+108
    for nm,meta,pct,col,spd,sc,eta in rows:
        b.append(torrent_row_full(mx,ry,mw,nm,meta,pct,col,spd,sc,eta))
        b.append(f'<line x1="{mx}" y1="{ry+40}" x2="{mx+mw}" y2="{ry+40}" stroke="{LINE}"/>')
        ry+=58
    b.append(txt(wx,wy+wh+34,"Full window — native sidebar, traffic lights, one health-bar per item.",12.5,FGS,400))
    return "".join(b)

# ============================================================
# helpers for phones
# ============================================================
def phone_frame(x,y,w=300,h=610,screen_fill="#0a1a14",dark=True):
    r=42
    out=[rrect(x,y,w,h,r,fill="#0c1712")]
    sx,sy,sw,sh=x+11,y+11,w-22,h-22
    out.append(rrect(sx,sy,sw,sh,r-10,fill=screen_fill))
    return out, (sx,sy,sw,sh)

# ============================================================
# 2) iOS + watchOS
# ============================================================
def ios_watch():
    b=[]
    px,py=120,140
    fr,(sx,sy,sw,sh)=phone_frame(px,py)
    b+=fr
    # notch
    b.append(rrect(px+ (300-100)/2, py+18, 100, 26, 13, fill="#05100b"))
    # status
    b.append(txt(sx+22,sy+34,"9:41",12,"#fff",700,MONO))
    b.append(txt(sx+sw-20,sy+34,"5G · 100%",11,"#cfe8dd",500,MONO,anchor="end"))
    # header
    b.append(mark(sx+18,sy+50,22)); b.append(txt(sx+50,sy+72,"Overview",21,"#fff",700))
    # remote badge
    b.append(rrect(sx+18,sy+86,sw-36,26,13,fill="none",stroke="#2b6f5c",sw=1))
    b.append(f'<circle cx="{sx+34}" cy="{sy+99}" r="4" fill="{B3}"/>')
    b.append(txt(sx+46,sy+103,"Connected · home-pi.local · WireGuard",10.5,"#7ef0cf",500,MONO))
    # total speed glass card
    cy=sy+122
    b.append(rrect(sx+18,cy,sw-36,64,16,fill="#12261e",stroke="#20362b",sw=1))
    b.append(txt(sx+34,cy+22,"TOTAL SPEED",10,"#9fd8c4",600,MONO,ls="1"))
    b.append(txt(sx+34,cy+50,"↓ 12.4",20,DOWN,800))
    b.append(txt(sx+120,cy+50,"MB/s",11,"#8fb3a7",500))
    b.append(txt(sx+178,cy+50,"↑ 2.1",20,UP,800))
    b.append(txt(sx+250,cy+50,"MB/s",11,"#8fb3a7",500))
    # transfer cards
    items=[("ubuntu-24.04.2.iso","62%",62,DOWN,"↓ 12.4 MB/s","14 min left","#7ef0cf"),
           ("debian-13-netinst.iso","seed",100,SEED,"↑ 2.1 MB/s","ratio 3.4","#11d4a0")]
    iy=cy+78
    for nm,tagr,pct,col,l,r,tc in items:
        b.append(rrect(sx+18,iy,sw-36,74,16,fill="#12261e",stroke="#20362b",sw=1))
        b.append(txt(sx+34,iy+24,nm,13,"#eaf4ee",600))
        b.append(txt(sx+sw-34,iy+24,tagr,10.5,tc,600,MONO,anchor="end"))
        b.append(bar(sx+34,iy+34,sw-68,pct,col,track="#1d3a2f"))
        b.append(txt(sx+34,iy+58,l,10.5,"#b3c8bf",400,MONO))
        b.append(txt(sx+sw-34,iy+58,r,10.5,"#b3c8bf",400,MONO,anchor="end"))
        iy+=86
    # tab bar (floating glass)
    tby=sy+sh-58
    b.append(rrect(sx+16,tby,sw-32,44,22,fill="#16342a",stroke="#264a3c",sw=1))
    tabs=["Overview","Transfers","Activity","Servers","Settings"]
    tw=(sw-32)/len(tabs)
    for i,t in enumerate(tabs):
        cx=sx+16+tw*i+tw/2
        on=(i==0)
        b.append(rrect(cx-8,tby+9,16,16,5,fill=B3 if on else "#5b7a6e"))
        b.append(txt(cx,tby+38,t,7.5,B3 if on else "#7f9a8e",600,anchor="middle"))
    b.append(txt(px,py+610+34,"iOS — installable PWA remote (App-Store-safe). Floating Liquid-Glass tab bar.",12.5,FGS,400))

    # ---- Apple Watch ----
    wx,wy=760,210
    ww,wh=250,320
    b.append(rrect(wx,wy,ww,wh,70,fill="#161616"))
    b.append(rrect(wx+26,wy+34,ww-52,wh-68,48,fill="#000000"))
    ix=wx+50; iy=wy+70
    b.append(mark(ix,iy-16,16)); b.append(txt(ix+24,iy-3,"Lidhra",13,B3,700))
    b.append(txt(wx+ww-50,iy-3,"10:09",12,"#9fb3a9",500,MONO,anchor="end"))
    b.append(txt(ix,iy+24,"TOTAL SPEED",9.5,"#9fd8c4",600,MONO,ls="1"))
    b.append(txt(ix,iy+52,"↓ 12.4",22,DOWN,800)); b.append(txt(ix+96,iy+52,"MB/s",11,"#8fb3a7"))
    b.append(txt(ix,iy+82,"↑ 2.1",22,UP,800)); b.append(txt(ix+96,iy+82,"MB/s",11,"#8fb3a7"))
    b.append(f'<line x1="{ix}" y1="{iy+100}" x2="{wx+ww-50}" y2="{iy+100}" stroke="#243a31"/>')
    b.append(txt(ix+30,iy+128,"3",22,"#fff",800,anchor="middle")); b.append(txt(ix+30,iy+146,"Active",9.5,"#8aa599",400,anchor="middle"))
    b.append(txt(wx+ww-80,iy+128,"5",22,SEED,800,anchor="middle")); b.append(txt(wx+ww-80,iy+146,"Seeding",9.5,"#8aa599",400,anchor="middle"))
    b.append(txt(wx+ww/2,wy+wh+34,"watchOS glance — the wrist is the fastest surface.",12.5,FGS,400,anchor="middle"))
    return "".join(b)

# ============================================================
# 3) Android — Material 3
# ============================================================
def android():
    b=[]
    px,py=120,140
    GREEN="#0d6b47"; CONT="#cdeadd"; SURF="#eef4f0"; ONS="#111c17"
    fr,(sx,sy,sw,sh)=phone_frame(px,py,screen_fill=SURF)
    b+=fr
    b.append(rrect(px+(300-70)/2,py+16,70,16,8,fill="#00000018"))
    b.append(txt(sx+22,sy+34,"9:41",12,ONS,700,MONO))
    b.append(txt(sx+sw-20,sy+34,"5G · 92%",11,"#3a4a42",500,MONO,anchor="end"))
    b.append(mark(sx+18,sy+50,22)); b.append(txt(sx+50,sy+72,"Lidhra",22,ONS,600))
    # chips
    chips=[("All",True),("Downloading",False),("Seeding",False),("Linux",False)]
    cx=sx+18
    for lab,on in chips:
        wchip=len(lab)*7.2+26
        b.append(rrect(cx,sy+88,wchip,28,14,fill=GREEN if on else CONT))
        b.append(txt(cx+wchip/2,sy+107,lab,11,"#fff" if on else "#0d4a34",500,anchor="middle"))
        cx+=wchip+8
    # total speed card
    cy=sy+128
    b.append(rrect(sx+16,cy,sw-32,66,22,fill="#ffffff"))
    b.append(txt(sx+34,cy+24,"TOTAL SPEED",10,GREEN,600,MONO,ls="1"))
    b.append(txt(sx+34,cy+52,"↓ 12.4",20,GREEN,800)); b.append(txt(sx+120,cy+52,"MB/s",11,"#3a4a42"))
    b.append(txt(sx+184,cy+52,"↑ 2.1",20,"#0a7d8a",800)); b.append(txt(sx+250,cy+52,"MB/s",11,"#3a4a42"))
    # cards
    items=[("ubuntu-24.04.2.iso","62%",62,GREEN,"↓ 12.4 MB/s","14 min"),
           ("debian-13-netinst.iso","seeding",100,SEED,"↑ 2.1 MB/s","ratio 3.4")]
    iy=cy+80
    for nm,tagr,pct,col,l,r in items:
        b.append(rrect(sx+16,iy,sw-32,80,22,fill="#ffffff"))
        b.append(txt(sx+34,iy+28,nm,14,ONS,600))
        b.append(txt(sx+sw-34,iy+28,tagr,11,GREEN,600,MONO,anchor="end"))
        b.append(bar(sx+34,iy+40,sw-68,pct,col,track="#d5e8de"))
        b.append(txt(sx+34,iy+64,l,11,"#3a4a42",400,MONO))
        b.append(txt(sx+sw-34,iy+64,r,11,"#3a4a42",400,MONO,anchor="end"))
        iy+=92
    # FAB
    b.append(rrect(sx+sw-78,sy+sh-140,58,58,19,fill="url(#lg)"))
    b.append(txt(sx+sw-49,sy+sh-102,"+",30,"#06251b",700,anchor="middle"))
    # nav bar
    nby=sy+sh-56
    b.append(rrect(sx,nby,sw,56,0,fill="#dcebe3"))
    tabs=[("Overview",True),("Transfers",False),("Servers",False),("Settings",False)]
    tw=sw/len(tabs)
    for i,(t,on) in enumerate(tabs):
        cxx=sx+tw*i+tw/2
        if on: b.append(rrect(cxx-16,nby+8,32,18,9,fill="#9fe0bf"))
        b.append(rrect(cxx-9,nby+11,18,12,4,fill="#0d4a34" if on else "#3a4a42"))
        b.append(txt(cxx,nby+42,t,8.5,"#0d4a34" if on else "#3a4a42",600 if on else 400,anchor="middle"))
    b.append(txt(px,py+610+34,"Android — Material 3 (Material You). Brand-green tonal palette, chips, FAB, nav bar.",12.5,FGS,400))

    # info card to the right
    ix,iy=560,200
    b.append(rrect(ix,iy,540,300,18,fill=CARD,stroke=LINE,sw=1))
    b.append(txt(ix+28,iy+44,"Full client — Play Store permits it",18,FG,800))
    lines=["Google Play allows both full clients and remotes.",
           "Lidhra Android is a Tauri Mobile app that can run the",
           "engine locally OR act as a remote for a desktop / Pi —",
           "a toggle in setup. Same Material UI either way."]
    for i,l in enumerate(lines):
        b.append(txt(ix+28,iy+80+i*26,l,14,FGS,400))
    tags=["Material 3","Dynamic color","Foreground service","Tauri Mobile"]
    tx=ix+28
    for t in tags:
        wt=len(t)*7.3+22
        b.append(rrect(tx,iy+200,wt,26,7,fill="#f0f6f3",stroke=LINE,sw=1))
        b.append(txt(tx+wt/2,iy+217,t,11.5,FGS,500,MONO,anchor="middle")); tx+=wt+10
    return "".join(b)

# ============================================================
# 4) Windows 11 — Fluent
# ============================================================
def windows():
    b=[]
    # tray flyout (left, bottom-anchored feel)
    px,py,pw=90,150,340
    b.append(rrect(px,py,pw,300,10,fill="#f4f8f6",stroke="#dbe6e0",sw=1))
    b.append(rrect(px,py,pw,300,10,fill="url(#glow)"))
    b.append(mark(px+16,py+14,16)); b.append(txt(px+42,py+27,"Lidhra",14,FG,700))
    b.append(rrect(px+pw-120,py+11,104,22,11,fill="#dbf3e8"))
    b.append(txt(px+pw-68,py+26,"↓ 12.4 ↑ 2.1",11,"#0b8f79",600,MONO,anchor="middle"))
    rows=[("ubuntu-24.04.2-desktop.iso","62% · 14 min",62,DOWN,"12.4 MB/s"),
          ("debian-13-netinst.iso","Seeding · 3.4",100,SEED,"↑ 2.1")]
    ry=py+48
    for nm,st,pct,col,spd in rows:
        b.append(rrect(px+10,ry,pw-20,58,6,fill="#ffffff"))
        b.append(rrect(px+20,ry+13,30,30,6,fill=col))
        b.append(txt(px+58,ry+22,nm,12.5,FG,600))
        b.append(txt(px+58,ry+38,st,10.5,FGM,400,MONO))
        b.append(txt(px+pw-20,ry+22,spd,11,col,600,MONO,anchor="end"))
        b.append(bar(px+58,ry+44,pw-88,pct,col))
        ry+=66
    b.append(f'<rect x="{px+14}" y="{ry+4}" width="{pw-28}" height="1" fill="{LINE}"/>')
    b.append(txt(px+18,ry+24,"Right-click tray → controls",12,FGS,400))
    b.append(rrect(px+pw-74,ry+10,58,24,5,fill="url(#lgh)")); b.append(txt(px+pw-45,ry+26,"Open",12,"#06251b",700,anchor="middle"))
    b.append(txt(px,py+300+34,"System-tray flyout — Fluent Acrylic, rounded, above the clock.",12.5,FGS,400))

    # window (right)
    wx,wy,ww,wh=520,150,780,470
    b.append(rrect(wx,wy,ww,wh,8,fill=CARD,stroke=LINE,sw=1))
    b.append(rrect(wx,wy,ww,40,8,fill="#f6faf8")); b.append(rrect(wx,wy+24,ww,16,0,fill=CARD))
    b.append(rrect(wx,wy,ww,wh,8,fill="none",stroke=LINE,sw=1))
    b.append(mark(wx+14,wy+9,16)); b.append(txt(wx+40,wy+25,"Lidhra",13,FG,700))
    for i,g in enumerate(["—","▢","✕"]):
        b.append(txt(wx+ww-70+i*24,wy+25,g,13,FGM,400,anchor="middle"))
    sbx=wx; sbw=190
    b.append(f'<rect x="{sbx}" y="{wy+40}" width="{sbw}" height="{wh-40}" fill="#f9fcfa"/>')
    b.append(f'<line x1="{sbx+sbw}" y1="{wy+40}" x2="{sbx+sbw}" y2="{wy+wh}" stroke="{LINE}"/>')
    side=[("LIBRARY",None,True,None),("Overview","12",False,True),("Transfers","3",False,False),
          ("Seeding","7",False,False),("TOOLS",None,True,None),("Activity",None,False,False),("Servers",None,False,False)]
    sy=wy+66
    for label,cnt,grp,sel in side:
        if grp:
            b.append(txt(sbx+20,sy,label,10,FGM,700,MONO,ls="1.4")); sy+=24; continue
        if sel: b.append(rrect(sbx+12,sy-15,sbw-24,26,7,fill="#dcf1e7"))
        b.append(txt(sbx+22,sy+2,"◆ "+label,12.5,"#0b8f79" if sel else FGS,600 if sel else 400))
        if cnt: b.append(txt(sbx+sbw-20,sy+2,cnt,10.5,FGM,400,MONO,anchor="end"))
        sy+=30
    mx=sbx+sbw+22; mw=ww-sbw-44
    b.append(rrect(mx,wy+58,64,26,6,fill="url(#lgh)")); b.append(txt(mx+32,wy+75,"+ Add",12,"#06251b",700,anchor="middle"))
    b.append(rrect(mx+72,wy+58,74,26,6,fill="#eef4f0",stroke=LINE,sw=1)); b.append(txt(mx+109,wy+75,"Pause all",11,FGS,400,anchor="middle"))
    b.append(txt(wx+ww-30,wy+75,"↓ 12.4 · ↑ 2.1 MB/s",11,FGM,500,MONO,anchor="end"))
    rows=[("ubuntu-24.04.2-desktop-amd64.iso","4.7 GB · 62% · 214 peers",62,DOWN,"12.4 MB/s",DOWN,"14m"),
          ("debian-13-netinst.iso","631 MB · seeding · ratio 3.4",100,SEED,"↑ 2.1",SEED,"∞")]
    ry=wy+108
    for nm,meta,pct,col,spd,sc,eta in rows:
        b.append(torrent_row_full(mx,ry,mw,nm,meta,pct,col,spd,sc,eta))
        b.append(f'<line x1="{mx}" y1="{ry+40}" x2="{mx+mw}" y2="{ry+40}" stroke="{LINE}"/>')
        ry+=58
    b.append(txt(wx,wy+wh+34,"Mica-backed window, Fluent controls, native min / max / close.",12.5,FGS,400))
    return "".join(b)

# ============================================================
# 5) Linux — GNOME desktop + phosh mobile
# ============================================================
def linux():
    b=[]
    ACC="#2ec27e"
    wx,wy,ww,wh=90,160,700,440
    b.append(rrect(wx,wy,ww,wh,12,fill=CARD,stroke=LINE,sw=1))
    b.append(rrect(wx,wy,ww,46,12,fill="#f6faf8")); b.append(rrect(wx,wy+30,ww,16,0,fill=CARD))
    b.append(rrect(wx,wy,ww,wh,12,fill="none",stroke=LINE,sw=1))
    b.append(rrect(wx+14,wy+11,60,24,7,fill="url(#lgh)")); b.append(txt(wx+44,wy+27,"+ Add",12,"#06251b",700,anchor="middle"))
    b.append(mark(wx+ww/2-52,wy+13,16)); b.append(txt(wx+ww/2-26,wy+29,"Lidhra",13,FG,700))
    for i,g in enumerate(["☰","▢","✕"]):
        b.append(txt(wx+ww-64+i*22,wy+29,g,13,FGM,400,anchor="middle"))
    # boxed list
    lx,ly,lw=wx+20,wy+64,ww-40
    b.append(rrect(lx,ly,lw,wh-84,12,fill="#ffffff",stroke=LINE,sw=1))
    rows=[("ubuntu-24.04.2-desktop-amd64.iso","Downloading · 62% · 214 peers",62,ACC,"12.4 MB/s",ACC,"14m"),
          ("debian-13-netinst.iso","Seeding · ratio 3.4",100,SEED,"↑ 2.1",SEED,"∞"),
          ("archlinux-2026.07.01-x86_64.iso","Paused · 88%",88,PAUSE,"—",FGM,"—")]
    ry=ly+30
    for i,(nm,meta,pct,col,spd,sc,eta) in enumerate(rows):
        b.append(torrent_row_full(lx+18,ry,lw-36,nm,meta,pct,col,spd,sc,eta))
        if i<len(rows)-1: b.append(f'<line x1="{lx}" y1="{ry+40}" x2="{lx+lw}" y2="{ry+40}" stroke="{LINE}"/>')
        ry+=58
    b.append(txt(wx,wy+wh+34,"GNOME / libadwaita — header-bar actions, boxed-list rows, green accent (#2EC27E).",12.5,FGS,400))

    # phosh mobile
    px,py=880,150
    fr,(sx,sy,sw,sh)=phone_frame(px,py,w=260,h=520,screen_fill="#151d18")
    b+=fr
    b.append(rrect(sx+8,sy+10,28,28,8,fill="#ffffff18")); b.append(txt(sx+22,sy+29,"☰",14,"#fff",400,anchor="middle"))
    b.append(mark(sx+sw/2-40,sy+10,18)); b.append(txt(sx+sw/2-14,sy+28,"Lidhra",14,"#fff",700))
    b.append(rrect(sx+sw-36,sy+10,28,28,8,fill="#ffffff18")); b.append(txt(sx+sw-22,sy+29,"+",16,"#fff",600,anchor="middle"))
    items=[("ubuntu-24.04.2.iso","62%",62,ACC,"↓ 12.4 MB/s · 14 min","#54e06a"),
           ("debian-13.iso","seed",100,SEED,"↑ 2.1 MB/s · ratio 3.4","#3ce0c0"),
           ("archlinux.iso","paused",88,PAUSE,"","#9aa8a0")]
    iy=sy+52
    for nm,tagr,pct,col,sub,tc in items:
        b.append(rrect(sx+14,iy,sw-28,64,14,fill="#ffffff10",stroke="#ffffff14",sw=1))
        b.append(txt(sx+28,iy+24,nm,12.5,"#e6efe9",600))
        b.append(txt(sx+sw-28,iy+24,tagr,10,tc,600,MONO,anchor="end"))
        b.append(bar(sx+28,iy+34,sw-56,pct,col,track="#26332d"))
        if sub: b.append(txt(sx+28,iy+56,sub,9.5,"#b7c4bd",400,MONO))
        iy+=72
    b.append(txt(px,py+520+34,"Linux mobile (phosh) — the same libadwaita view, adaptively collapsed.",12.5,FGS,400))
    return "".join(b)

# ---- write files ----
pages=[
 ("01-macos.svg",      artboard("macOS","Liquid Glass · menu-bar + window","01 / 05  ·  Lidhra app designs", macos(),   "Lidhra — macOS · concept mockup · not affiliated with qBittorrent")),
 ("02-ios-watchos.svg",artboard("iOS + watchOS","Liquid Glass · PWA remote","02 / 05  ·  Lidhra app designs", ios_watch(),"Lidhra — iOS + watchOS · App-Store-safe remote")),
 ("03-android.svg",    artboard("Android","Material 3 · Material You","03 / 05  ·  Lidhra app designs", android(), "Lidhra — Android · full client (Play Store permitted)")),
 ("04-windows.svg",    artboard("Windows 11","Fluent 2 · tray + Mica window","04 / 05  ·  Lidhra app designs", windows(), "Lidhra — Windows 11 · Fluent design")),
 ("05-linux.svg",      artboard("Linux","libadwaita · GNOME + phosh","05 / 05  ·  Lidhra app designs", linux(),   "Lidhra — Linux desktop + mobile · adaptive libadwaita")),
]
for fn,svg in pages:
    open(os.path.join(OUT,fn),"w").write(svg)
    print("wrote",fn,len(svg),"bytes")
print("done")
