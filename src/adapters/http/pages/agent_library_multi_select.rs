//! Reusable multi-select cards for provisioning one channel per library agent.

use super::*;

/// The agent-library workspace's own behaviour. It is bundled into `/assets/app.js` rather than
/// shipped with the page: the strict `script-src 'self'` CSP blocks inline scripts, and the
/// per-page `script` hook that used to carry it was silently discarded by the shell.
pub(crate) const AGENT_LIBRARY_SCRIPT: &str = r#"
function libraryPayload(form){const data=new FormData(form);let config=null;try{config=data.get('config_json')?JSON.parse(data.get('config_json')):null}catch(e){alert('Config must be valid JSON');throw e}return {name:data.get('name'),slug:data.get('slug'),provider:data.get('provider')||null,model:data.get('model')||null,system_prompt:data.get('system_prompt')||null,description:data.get('description')||null,config_json:config,avatar_url:data.get('avatar_url')||null}}
async function libraryRequest(url,method,body){const response=await fetch(url,{method,headers:{'content-type':'application/json'},body:body?JSON.stringify(body):undefined});if(!response.ok)throw new Error(await response.text());return response.status===204?null:response.json()}
async function createLibraryAgent(event){event.preventDefault();try{await libraryRequest('/api/agent-library','POST',libraryPayload(event.target));location.reload()}catch(e){alert(e.message)}}
async function saveLibraryAgent(event,id){event.preventDefault();try{await libraryRequest('/api/agent-library/'+id,'PUT',libraryPayload(event.target));location.reload()}catch(e){alert(e.message)}}
async function deleteLibraryAgent(id){if(!confirm('Delete this library agent?'))return;try{await libraryRequest('/api/agent-library/'+id,'DELETE');location.reload()}catch(e){alert(e.message)}}
"#;

/// A self-contained library picker. Checkboxes are presentation controls; the hidden input is the
/// stable, comma-separated value submitted by ordinary URL-encoded forms.
pub fn agent_library_multi_select(agents: &[Agent], selected: &[Uuid], input_name: &str) -> String {
    let library = agents
        .iter()
        .filter(|agent| agent.is_library())
        .collect::<Vec<_>>();
    if library.is_empty() {
        return String::new();
    }

    let cards = library
        .iter()
        .map(|agent| {
            let description = agent.description.as_deref().unwrap_or("Ready-to-use agent");
            format!(
                r##"<label class="flex cursor-pointer items-start gap-3 rounded-box border border-base-300 bg-base-200/40 p-4 hover:border-primary">
                    <input type="checkbox" value="{id}" class="checkbox checkbox-primary mt-1"{checked}
                        data-action="library-multi-select">
                    <span class="min-w-0"><span class="block font-semibold">{name}</span><span class="block font-mono text-xs opacity-60">{slug}</span><span class="mt-1 block text-sm opacity-70">{description}</span></span>
                </label>"##,
                id = agent.id,
                checked = if selected.contains(&agent.id) { " checked" } else { "" },
                name = escape_html_text(&agent.name),
                slug = escape_html_text(&agent.slug),
                description = escape_html_text(description),
            )
        })
        .collect::<String>();
    let value = selected
        .iter()
        .map(Uuid::to_string)
        .collect::<Vec<_>>()
        .join(",");

    format!(
        r##"<section class="space-y-3" data-library-multi-select>
            <input type="hidden" name="{input_name}" value="{value}">
            <div><h2 class="font-semibold">Start from the agent library</h2><p class="text-sm opacity-60">Select any number. Each agent gets a channel with the same name and email slug.</p></div>
            <div class="grid grid-cols-1 gap-3 md:grid-cols-2">{cards}</div>
        </section>"##,
        input_name = escape_html_text(input_name),
    )
}
