"use strict";
(() => {
  const root = document.getElementById("setup");
  if (!root) return;
  const manage = root.dataset.manage === "true";
  const status = document.getElementById("setup-status");
  const error = document.getElementById("setup-error");
  const result = document.getElementById("setup-result");
  let selected = null, pending = null, busy = false;
  const cursors = {}, domains = new Map(), campaigns = new Map(), ads = new Map();
  const storeLists = ["domains","installations","credentials","campaigns","ads","links"];
  root.querySelectorAll("[data-manage]").forEach(node => {node.hidden = !manage;});
  document.getElementById("setup-readonly").hidden = manage;
  function element(tag, text) { const node = document.createElement(tag); node.textContent = text; return node; }
  function failure(message, retryable = false) {
    error.hidden = false;
    error.querySelector("p").textContent = message;
    document.getElementById("setup-retry").hidden = !retryable;
  }
  function button(label, action) { const b = element("button", label); b.type = "button"; b.disabled = busy; b.addEventListener("click", action); return b; }
  const headers = {"X-Company-ID":root.dataset.company};
  async function read(path) {
    const response = await fetch(path, {headers, cache:"no-store"});
    if (!response.ok) throw new Error(response.status === 401 ? "Your session expired. Sign in with Openbot again." : "Setup could not load. Try refreshing the page.");
    return response.json();
  }
  async function list(kind, more = false) {
    const storeAtStart = selected?.id;
    const query = new URLSearchParams({limit:"25"});
    if (kind !== "stores") query.set("store_id", selected.id);
    if (more && cursors[kind]) query.set("cursor", cursors[kind]);
    const page = await read(`/v1/${kind}?${query}`);
    if (kind !== "stores" && selected?.id !== storeAtStart) return;
    const target = document.getElementById(`${kind}-list`);
    if (!more) {target.replaceChildren(); if (kind === "domains") domains.clear(); if (kind === "campaigns") campaigns.clear(); if (kind === "ads") ads.clear();}
    for (const item of page.items) {
      const row = document.createElement("li");
      if (kind === "stores") {
        row.append(button(`${item.name} (${item.enabled ? "enabled" : "disabled"})`, () => select(item).catch(e => failure(e.message))));
      } else if (kind === "domains") {
        domains.set(item.id, item);
        row.append(element("p", `${item.host}: ${item.status}${item.verified_until ? `; approval ends ${item.verified_until}` : ""}`));
        if (manage && item.status !== "revoked") {
          row.append(button("Verify domain", () => send(`/v1/domains/${item.id}/verify`, "POST")));
          row.append(button("Revoke domain", () => send(`/v1/domains/${item.id}/revoke`, "POST")));
        }
      } else if (kind === "installations") {
        row.append(element("p", `${item.name}: ${item.enabled ? "enabled" : "disabled"}`));
        row.append(element("code", `Public installation ID: ${item.id}`));
        if (manage) row.append(button(item.enabled ? "Disable installation" : "Enable installation", () => send(`/v1/installations/${item.id}`, "PATCH", {enabled:!item.enabled})));
      } else if (["campaigns","ads","links"].includes(kind)) {
        catalogRow(row, kind, item);
      } else {
        row.append(element("p", `${item.label}: ${item.scopes.join(", ")}; created ${item.created_at}; expires ${item.expires_at || "never"}; revoked ${item.revoked_at || "no"}; last used ${item.last_used_at || "never"}`));
        row.append(element("code", `Key ID: ${item.id}`));
        if (manage && !item.revoked_at) {
          row.append(button("Revoke key", () => send(`/v1/credentials/${item.id}/revoke`, "POST")));
          row.append(button("Rotate key", () => {
            const input = document.getElementById("rotation-overlap");
            if (!input.reportValidity()) return;
            send(`/v1/credentials/${item.id}/rotate`, "POST", {overlap_seconds:Number(input.value)});
          }));
        }
      }
      target.append(row);
    }
    if (!target.children.length) target.append(element("li", "No entries yet."));
    cursors[kind] = page.next_cursor;
    document.getElementById(`${kind}-more`).hidden = !page.next_cursor;
    if (kind === "campaigns") choices("ad-campaign",campaigns,"Choose a campaign");
    if (kind === "ads") choices("link-ad",ads,"Choose an ad");
    if (kind === "domains") {
      const select = document.querySelector('#installation-create select');
      const previous = select.value;
      select.replaceChildren(element("option", "Choose a verified domain"));
      select.firstChild.value = "";
      for (const domain of domains.values()) if (domain.status === "verified") {
        const option = element("option", domain.host); option.value = domain.id; select.append(option);
      }
      select.value = previous;
    }
  }
  function choices(id, entries, placeholder) {
    const select = document.getElementById(id), previous = select.value;
    select.replaceChildren(element("option", placeholder)); select.firstChild.value = "";
    for (const item of entries.values()) if (item.enabled) {
      const option = element("option", item.name); option.value = item.id; select.append(option);
    }
    select.value = previous;
  }
  function catalogRow(row, kind, item) {
    if (kind === "campaigns") campaigns.set(item.id,item);
    if (kind === "ads") ads.set(item.id,item);
    row.append(element("p", `${item.name || item.placement}: ${item.enabled ? "enabled" : "disabled"}`));
    if (kind === "ads") row.append(element("p", `Product reference: ${item.advertised_product_id || "none"}`));
    if (kind === "links") {
      const url = new URL(`/r/${item.code}`,location.origin).href;
      const link = element("a",url); link.href = url; link.target = "_blank"; link.rel = "noopener noreferrer";
      row.append(link,element("p",`Destination: ${item.destination}; revision ${item.revision}`));
    }
    if (!manage) return;
    const form = document.createElement("form");
    form.setAttribute("aria-label",`Edit ${kind === "links" ? "link" : kind === "ads" ? "ad" : "campaign"}: ${item.name || item.placement}`);
    function input(name, label, type, max, value, required = true) {
      const id = `${kind}-${item.id}-${name}`;
      const caption = element("label",label); caption.htmlFor = id;
      const field = document.createElement("input"); field.id = id; field.name = name; field.type = type; field.maxLength = max; field.required = required; field.value = value || "";
      form.append(caption,field);
    }
    if (kind === "links") {
      input("destination","Destination URL","url",2048,item.destination);
      input("placement","Placement","text",120,item.placement);
    } else input("name",kind === "ads" ? "Ad name" : "Campaign name","text",120,item.name);
    if (kind === "ads") input("advertised_product_id","Advertised product reference (optional)","text",128,item.advertised_product_id,false);
    const save = element("button","Save changes"); save.type = "submit"; save.disabled = busy; form.append(save);
    form.addEventListener("submit", event => {
      event.preventDefault(); if (!form.reportValidity()) return;
      const data = Object.fromEntries(new FormData(form));
      if (kind === "ads" && !data.advertised_product_id) data.advertised_product_id = null;
      send(`/v1/${kind}/${item.id}`,"PATCH",data);
    });
    row.append(form,button(item.enabled ? "Disable" : "Enable",() => send(`/v1/${kind}/${item.id}`,"PATCH",{enabled:!item.enabled})));
  }
  async function select(store) {
    selected = store; result.replaceChildren(); result.hidden = true;
    document.getElementById("selected-store").hidden = false;
    document.getElementById("store-heading").textContent = store.name;
    const form = document.getElementById("store-edit");
    form.elements.name.value = store.name; form.elements.timezone.value = store.timezone; form.elements.enabled.checked = store.enabled;
    await Promise.all(storeLists.map(kind => list(kind)));
  }
  async function refresh() {
    await list("stores");
    if (selected) {
      selected = await read(`/v1/stores/${selected.id}`);
      document.getElementById("store-heading").textContent = selected.name;
      await Promise.all(storeLists.map(kind => list(kind)));
    }
  }
  function show(value) {
    result.replaceChildren(); result.hidden = false;
    if (value.secret) {
      result.append(element("h4", "Copy this server API key now"), element("p", "It will not be shown again. Store it securely on your server."), element("code", value.secret));
      result.append(button("I have saved the key", () => {result.replaceChildren(); result.hidden = true;}));
    } else if (value.secret_available === false) {
      result.append(element("p", "This operation already completed. The secret cannot be recovered. Revoke this key, then create a replacement."), element("code", value.credential.id));
      result.append(button("Revoke unrecoverable key", () => send(`/v1/credentials/${value.credential.id}/revoke`, "POST")));
    } else if (value.record_name) {
      result.append(element("h4", "Publish this DNS TXT record"), element("p", `Name: ${value.record_name}`), element("code", value.record_value), element("p", "Keep the record for later re-verification. Then choose Verify domain."));
    } else { result.append(element("p", "Saved.")); }
    result.focus();
  }
  async function execute() {
    if (busy || !pending) return;
    busy = true; root.querySelectorAll("button").forEach(button => {button.disabled = true;}); error.hidden = true; status.textContent = "Saving…";
    const operation = pending;
    try {
      const response = await fetch(operation.path, {method:operation.method, headers:{...headers,"X-CSRF-Token":root.dataset.csrf,"Content-Type":"application/json","Idempotency-Key":operation.key,"Idempotency-Created-At":operation.time},body:operation.body});
      if (!response.ok) {
        const messages = {400:"Check the fields. For domain verification, publish the TXT value and ensure the host resolves to public addresses.",401:"Your session expired. Sign in with Openbot again.",403:"You cannot make this change, or the company/store is disabled.",404:"The scoped resource was not found.",409:"The operation conflicts with existing state. Inspect the current setup before submitting a new operation.",410:"The resource or original retry window expired. Inspect current state before a new operation.",413:"The request is too large.",429:"Setup is busy. Wait briefly, then retry.",503:"The operation could not be confirmed. Retry the same request."};
        failure(messages[response.status] || "The operation could not be confirmed. Retry the same request.", [429,503].includes(response.status));
        status.textContent = "Change not confirmed."; return;
      }
      const value = await response.json(); pending = null; show(value); status.textContent = "Saved.";
      await refresh();
    } catch { failure("The response could not be confirmed. Retry the same request if offered; otherwise refresh the current setup.", pending !== null); status.textContent = "Connection interrupted."; }
    finally {busy = false; root.querySelectorAll("button").forEach(button => {button.disabled = false;});}
  }
  function send(path, method, body) {
    if (!manage || busy) return;
    if (pending) {failure("Resolve or dismiss the previous request before starting a different operation.",true);return;}
    result.replaceChildren(); result.hidden = true;
    pending = {path,method,body:body === undefined ? undefined : JSON.stringify(body),key:crypto.randomUUID(),time:new Date().toISOString()};
    execute();
  }
  function form(id, operation) {
    document.getElementById(id).addEventListener("submit", event => {event.preventDefault(); if (event.currentTarget.reportValidity()) operation(new FormData(event.currentTarget));});
  }
  form("store-create", data => send("/v1/stores","POST",{name:data.get("name"),timezone:data.get("timezone")}));
  form("store-edit", data => send(`/v1/stores/${selected.id}`,"PATCH",{name:data.get("name"),timezone:data.get("timezone"),enabled:data.has("enabled")}));
  form("domain-create", data => send("/v1/domains","POST",{store_id:selected.id,host:data.get("host")}));
  form("installation-create", data => send("/v1/installations","POST",{store_id:selected.id,domain_id:data.get("domain_id"),name:data.get("name")}));
  form("credential-create", data => {
    const scopes = data.getAll("scopes"); if (!scopes.length) {failure("Choose at least one allowed operation.");return;}
    send("/v1/credentials","POST",{store_id:selected.id,label:data.get("label"),scopes,expires_at:data.get("expires_at") ? new Date(data.get("expires_at")).toISOString() : null});
  });
  form("campaign-create", data => send("/v1/campaigns","POST",{store_id:selected.id,name:data.get("name")}));
  form("ad-create", data => send("/v1/ads","POST",{store_id:selected.id,campaign_id:data.get("campaign_id"),name:data.get("name"),advertised_product_id:data.get("advertised_product_id") || null}));
  form("link-create", data => send("/v1/links","POST",{store_id:selected.id,ad_id:data.get("ad_id"),destination:data.get("destination"),placement:data.get("placement")}));
  document.getElementById("setup-retry").addEventListener("click", execute);
  document.getElementById("setup-dismiss").addEventListener("click", () => {if (!busy) {pending = null;error.hidden = true;}});
  for (const kind of ["stores",...storeLists]) document.getElementById(`${kind}-more`).addEventListener("click", () => list(kind,true).catch(e => failure(e.message)));
  window.addEventListener("pagehide", () => {result.replaceChildren();pending = null;});
  list("stores").catch(e => failure(e.message));
})();
