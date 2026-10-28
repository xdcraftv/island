import { html, render, useState, useEffect } from './vendor/htm-preact.js';
const T = window.__TAURI__,
  store = { wx: null, sys: null, clips: [] },
  subs = new Set();
const set = (p) => {
  Object.assign(store, p);
  subs.forEach((f) => f());
};
const useStore = () => {
  const [, n] = useState(0);
  useEffect(() => {
    const f = () => n((x) => x + 1);
    subs.add(f);
    return () => subs.delete(f);
  }, []);
  return store;
};
const KIND = (c) =>
  c == 0 ? 'sun' : c < 3 ? 'part' : c < 51 ? 'cloud' : c < 95 ? 'rain' : 'storm';
const LBL = {
  sun: 'Clear',
  part: 'Partly cloudy',
  cloud: 'Cloudy',
  rain: 'Rain',
  storm: 'Thunderstorm'
};
const CL = 'M7 18a4 4 0 0 1-.5-8 5.5 5.5 0 0 1 10.6 1.2A3.5 3.5 0 0 1 17 18z';
const Icon = ({ k, s = 16 }) =>
  html`<svg viewBox="0 0 24 24" width=${s} height=${s} fill="none" stroke="#fff" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${
    {
      sun: html`<g stroke="#ffd54a"><circle cx="12" cy="12" r="4.5" fill="#ffd54a"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2"/></g>`,
      part: html`<circle cx="9" cy="8" r="3.5" fill="#ffd54a" stroke="none"/><path d=${CL}/>`,
      cloud: html`<path d=${CL}/>`,
      rain: html`<g transform="translate(0,-3)"><path d=${CL}/></g><path d="M8 18l-1 3M12 18l-1 3M16 18l-1 3" stroke="#7aa5ff"/>`,
      storm: html`<g transform="translate(0,-3)"><path d=${CL}/></g><path d="M12.5 14l-2.5 4h3l-2 4" stroke="#ffd54a"/>`
    }[k]
  }</svg>`;
const Idle = () => {
  const { wx } = useStore();
  return html`<${Icon} k=${wx ? wx.k : 'cloud'} s=${15}/>`;
};
const Weather = () => {
  const { wx } = useStore(),
    [t, setT] = useState(new Date());
  useEffect(() => {
    const i = setInterval(() => setT(new Date()), 1000);
    return () => clearInterval(i);
  }, []);
  return html`<div class="wx"><${Icon} k=${wx ? wx.k : 'cloud'} s=${58}/><div><div class="big">${t.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</div>
 <small>${wx ? wx.temp + '° · ' + LBL[wx.k] + ' · ' + (wx.loc || 'Karanganyar') : 'Weather needs internet'}</small></div>
 <div class="fc">${(wx ? wx.days : []).map((d) => html`<div><span>${d.d}</span><${Icon} k=${d.k} s=${26}/><span>${d.hi}°/${d.lo}°</span></div>`)}</div></div>`;
};
const kb = (b) =>
  b > 1048576 ? (b / 1048576).toFixed(1) + ' MB/s' : Math.round(b / 1024) + ' KB/s';
const Gauge = ({ n, v, c }) => {
  const k = 138.2,
    ok = Number.isFinite(v) && v >= 0,
    p = ok ? Math.min(100, v) : 0;
  return html`<div class="g"><svg viewBox="0 0 56 56"><circle cx="28" cy="28" r="22" fill="none" stroke="#ffffff1f" stroke-width="5"/>
 <circle cx="28" cy="28" r="22" fill="none" stroke=${c} stroke-width="5" stroke-linecap="round" stroke-dasharray=${(k * p) / 100 + ' ' + k} transform="rotate(-90 28 28)" style="transition:stroke-dasharray .5s"/>
 <text x="28" y="33" text-anchor="middle" fill="#fff" font-size="14" font-weight="600">${ok ? Math.round(p) + '%' : 'n/a'}</text></svg><span>${n}</span></div>`;
};
const Sys = () => {
  const { sys: s } = useStore();
  if (!s) return html`<div class="empty2">Hardware stats show in the desktop app.</div>`;
  const ic = (c, d) =>
    html`<svg viewBox="0 0 24 24" fill="none" stroke=${c} stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d=${d}/></svg>`;
  const ramPct = s.ram_total > 0 ? (s.ram_used / s.ram_total) * 100 : 0;
  return html`<div class="gs"><${Gauge} n="CPU" v=${s.cpu} c="#7aa5ff"/><${Gauge} n="RAM" v=${ramPct} c="#b18cff"/><${Gauge} n="GPU" v=${s.gpu} c="#34d27b"/><${Gauge} n="Disk" v=${s.disk} c="#ffb74d"/></div>
 <div class="net"><span>${ic('#7aa5ff', 'M12 5v14M6 13l6 6 6-6')}${kb(s.down)}</span><span>${ic('#34d27b', 'M12 19V5M6 11l6-6 6 6')}${kb(s.up)}</span>${
   s.bat >= 0
     ? html`<span>${ic(s.charging ? '#ffd54a' : '#34d27b', 'M6 7h11a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2zm15 4v2')}${s.bat}%${s.charging ? ' ⚡' : ''}</span>`
     : ''
 }</div>`;
};
const Clips = () => {
  const { clips } = useStore(),
    [copied, setCopied] = useState(null);
  const copy = (t, i) => {
    if (!t) return;
    navigator.clipboard.writeText(t);
    setCopied(i);
    setTimeout(() => setCopied(null), 900);
  };
  const togglePin = (e, item) => {
    e.stopPropagation();
    item.pin = !item.pin;
    const sorted = [...store.clips].sort((a, b) => (b.pin ? 1 : 0) - (a.pin ? 1 : 0));
    set({ clips: sorted });
  };
  const clearUnpinned = () => set({ clips: store.clips.filter((c) => c.pin) });
  return html`<div class="cl-wrap">${
    clips.length
      ? html`<div class="cl-hd"><span>Clipboard</span><button onClick=${clearUnpinned}>Clear</button></div>
        <div class="cl">${clips.map(
          (c, i) =>
            html`<div class=${'ci' + (c.pin ? ' pinned' : '')} title="Click to copy" onClick=${() => copy(c.t, i)}>
              <span class="ci-t">${copied === i ? '✓ Copied!' : c.t}</span>
              <button class="pin" title=${c.pin ? 'Unpin' : 'Pin'} onClick=${(e) => togglePin(e, c)}>${c.pin ? '★' : '☆'}</button>
            </div>`
        )}</div>`
      : html`<div class="empty2">Copy some text and it appears here.</div>`
  }</div>`;
};
const $ = (id) => document.getElementById(id);
render(html`<${Idle}/>`, $('wxIdle'));
render(html`<${Weather}/>`, $('wxPane'));
render(html`<${Sys}/>`, $('sysPane'));
render(html`<${Clips}/>`, $('clipPane'));
const D = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
let wxLat = -7.5972,
  wxLng = 110.9553;
async function loadWx() {
  try {
    const r = await (
      await fetch(
        `https://api.open-meteo.com/v1/forecast?latitude=${wxLat}&longitude=${wxLng}&current=temperature_2m,weather_code&daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=4`
      )
    ).json();
    set({
      wx: {
        temp: Math.round(r.current.temperature_2m),
        k: KIND(r.current.weather_code),
        days: r.daily.time
          .slice(1)
          .map((t, i) => ({
            d: D[new Date(t + 'T12:00').getDay()],
            k: KIND(r.daily.weather_code[i + 1]),
            hi: Math.round(r.daily.temperature_2m_max[i + 1]),
            lo: Math.round(r.daily.temperature_2m_min[i + 1])
          }))
      }
    });
  } catch (e) {}
}
window.addEventListener('loc', (e) => {
  wxLat = e.detail.lat;
  wxLng = e.detail.lng;
  loadWx();
});
loadWx();
setInterval(loadWx, 1200000);
document.querySelector('.tabs').addEventListener(
  'wheel',
  (e) => {
    e.currentTarget.scrollLeft += e.deltaY;
  },
  { passive: true }
);
if (T) {
  T.event.listen('sys', (e) => set({ sys: e.payload }));
  T.event.listen('clip', (e) => {
    const p = e.payload;
    let it;
    if (p.text != null) it = { t: p.text, pin: false };
    else {
      const c = document.createElement('canvas');
      c.width = p.w;
      c.height = p.h;
      c.getContext('2d').putImageData(
        new ImageData(
          Uint8ClampedArray.from(atob(p.rgba), (ch) => ch.charCodeAt(0)),
          p.w,
          p.h
        ),
        0,
        0
      );
      window.dispatchEvent(new CustomEvent('clipimg', { detail: c.toDataURL() }));
      return;
    }
    const existing = store.clips.find((x) => x.t === it.t);
    if (existing) it.pin = existing.pin;
    const next = [it, ...store.clips.filter((x) => x.t !== it.t)]
      .sort((a, b) => (b.pin ? 1 : 0) - (a.pin ? 1 : 0))
      .slice(0, 15);
    set({ clips: next });
  });
}
