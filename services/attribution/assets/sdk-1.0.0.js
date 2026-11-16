/* Openbot Attribution SDK 1.0.0. Explicit consent; no cookies or customer data. */
"use strict";
(() => {
  const service = new URL(document.currentScript.src).origin;
  const uuid = value => typeof value === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value) && value !== "00000000-0000-0000-0000-000000000000";
  const lifetime = 30 * 24 * 60 * 60 * 1000;
  function create(installationId) {
    if (!uuid(installationId)) throw new TypeError("A valid public installation ID is required");
    installationId = installationId.toLowerCase();
    const prefix = `ob-attribution:1:${installationId}:click:`, withdrawal = `ob-attribution:1:${installationId}:withdrawal`;
    const page = new URL(location.href), parameters = page.searchParams.getAll("ob_click");
    let captured = parameters.length === 1 && uuid(parameters[0]) ? parameters[0].toLowerCase() : null;
    if (parameters.length) {page.searchParams.delete("ob_click"); history.replaceState(history.state,"",page);}
    let consent = false, grantedAt = 0, generation = new AbortController(), pending = 0;
    const memory = new Map(), observers = new Set();
    function storage(action) {try {return action(localStorage);} catch {return undefined;}}
    function keys(store) {
      const found = [];
      for (let i=0;i<store.length;i++) {const key=store.key(i);if(key?.startsWith(prefix)) found.push(key);}
      return found;
    }
    function clearObservers() {for (const stop of [...observers]) stop();}
    function withdraw(broadcast) {
      consent = false; captured = null; memory.clear(); generation.abort(); clearObservers();
      storage(store => {for (const key of keys(store)) store.removeItem(key);if(broadcast) store.setItem(withdrawal,String(Date.now()));});
    }
    function historyEntries() {
      if (!consent) return [];
      const sampled=[];
      storage(store => {
        if (Number(store.getItem(withdrawal))>grantedAt) {withdraw(false);return;}
        for (const key of keys(store)) {
          try {
            const raw=store.getItem(key);
            if (!raw || raw.length>128) {store.removeItem(key);continue;}
            const item=JSON.parse(raw), readAt=Date.now();
            if (!uuid(item.id) || key!==prefix+item.id || !Number.isFinite(item.captured_at) || item.captured_at>readAt || readAt-item.captured_at>lifetime) {store.removeItem(key);continue;}
            sampled.push(key);
            const old=memory.get(item.id);memory.set(item.id,old ? Math.min(old,item.captured_at) : item.captured_at);
          } catch {store.removeItem(key);}
        }
      });
      if (!consent) return [];
      const now=Date.now();
      const retained=[...memory].filter(([,time])=>time<=now && now-time<=lifetime).sort((a,b)=>b[1]-a[1] || b[0].localeCompare(a[0])).slice(0,32);
      memory.clear();for(const [id,time] of retained) memory.set(id,time);
      // Only evict candidates considered by this read. Another tab may have
      // persisted a new click since then; its own read will enforce the bound.
      storage(store => {for(const key of sampled) if(!memory.has(key.slice(prefix.length))) store.removeItem(key);});
      return retained.map(([id])=>id);
    }
    function pause(ms,signal) {
      return new Promise(resolve => {
        if(signal.aborted) {resolve(false);return;}
        const stop=()=>{clearTimeout(timer);resolve(false);};
        const timer=setTimeout(()=>{signal.removeEventListener("abort",stop);resolve(true);},ms);
        signal.addEventListener("abort",stop,{once:true});
      });
    }
    async function send(kind,clickId,linkId) {
      if(!consent || pending>=32 || (!clickId && !linkId)) return false;
      pending++;
      const signal=generation.signal;
      try {
        const payload=JSON.stringify({installation_id:installationId,event_id:crypto.randomUUID(),kind,click_id:clickId,link_id:linkId,observed_at:new Date().toISOString(),consent:true});
        for(let attempt=0;attempt<3 && consent && !signal.aborted;attempt++) {
          const request=new AbortController(), stop=()=>request.abort();signal.addEventListener("abort",stop,{once:true});
          const timeout=setTimeout(stop,10000);let delay=1000*2**attempt;
          try {
            const response=await fetch(`${service}/v1/events`,{method:"POST",mode:"cors",credentials:"omit",referrerPolicy:"no-referrer",cache:"no-store",headers:{"Content-Type":"application/json"},body:payload,signal:request.signal});
            if(response.status===204) return true;
            if(![429,503].includes(response.status)) return false;
            const retry=Number(response.headers.get("Retry-After"));
            if(Number.isFinite(retry) && retry>0) delay=Math.max(delay,Math.min(retry*1000,60000));
          } catch {if(signal.aborted) return false;}
          finally {clearTimeout(timeout);signal.removeEventListener("abort",stop);}
          if(attempt<2 && !await pause(delay,signal)) return false;
        }
        return false;
      } catch {return false;} finally {pending--;}
    }
    function landing() {const candidates=historyEntries();if(candidates.length) void send("landing_page_load",captured || candidates[0],null);}
    function setConsent(value) {
      if(typeof value!=="boolean") throw new TypeError("Consent must be an explicit boolean");
      if(!value) {withdraw(true);return;}
      if(consent) return;
      consent=true;grantedAt=Date.now();generation=new AbortController();
      historyEntries();
      if(captured && consent) {
        const time=memory.get(captured) ?? Date.now();memory.set(captured,time);
        storage(store=>store.setItem(prefix+captured,JSON.stringify({id:captured,captured_at:time})));
      }
      landing();
    }
    function handoff() {const candidate_click_ids=historyEntries();return {consent,candidate_click_ids};}
    function observeVisible(element,linkId) {
      if(!uuid(linkId)) throw new TypeError("A valid placement link ID is required");
      if(!consent || !(element instanceof Element) || !window.IntersectionObserver) return ()=>{};
      let intersecting=false,timer=null,finished=false;
      const cancel=()=>{if(timer!==null) clearTimeout(timer);timer=null;};
      const stop=()=>{finished=true;cancel();observer.disconnect();document.removeEventListener("visibilitychange",visibility);observers.delete(stop);};
      const arm=()=>{cancel();if(intersecting && !finished && consent && document.visibilityState==="visible") timer=setTimeout(()=>{if(consent && intersecting && document.visibilityState==="visible") {void send("visible_placement",null,linkId.toLowerCase());stop();}},1000);};
      const visibility=()=>arm();
      const observer=new IntersectionObserver(entries=>{for(const entry of entries) if(entry.target===element) {intersecting=entry.isIntersecting && entry.intersectionRatio>=0.5;arm();}},{threshold:[0,0.5]});
      observers.add(stop);document.addEventListener("visibilitychange",visibility);observer.observe(element);return stop;
    }
    window.addEventListener("storage",event=>{if(event.key===withdrawal && consent) withdraw(false);});
    window.addEventListener("pagehide",()=>{generation.abort();clearObservers();});
    window.addEventListener("pageshow",event=>{if(event.persisted && consent) {historyEntries();if(consent) {generation=new AbortController();landing();}}});
    return Object.freeze({setConsent,handoff,observeVisible});
  }
  Object.defineProperty(window,"OpenbotAttribution",{value:Object.freeze({version:"1.0.0",create}),writable:false});
})();
