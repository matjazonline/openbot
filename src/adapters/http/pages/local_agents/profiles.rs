use super::*;

pub(super) fn render(view: &Page<'_>) -> String {
    let base = base(view.company.id);
    let rows=view.profiles.entries.iter().map(|profile|format!(r#"<li class="flex flex-wrap items-center gap-3"><a class="link" href="{link}">{name}</a><span>{provider} · {model} · project {project}</span><code class="select-all text-xs">{id}</code>{delete}</li>"#,
        id=profile.id,name=text(&profile.name),provider=text(&enum_value(&profile.selection.provider)),model=text(profile.selection.model.as_str()),project=text(profile.selection.project_id.as_str()),
        link=attr(&navigation(view,&format!("profile={}",profile.id))),
        delete=form(&format!("{base}/profiles/{}/delete",profile.id),"","Delete unused profile"))).collect::<String>();
    let next = if view.profiles.truncated {
        view.profiles
            .entries
            .last()
            .map_or_else(String::new, |profile| {
                format!(
                    r#"<a class="link" href="{}">More profiles</a>"#,
                    attr(&navigation(view, &format!("after_profile={}", profile.id)))
                )
            })
    } else {
        String::new()
    };
    format!(
        r#"<section class="space-y-4"><h2 class="text-xl font-semibold">Local agent profiles</h2><p>Profiles select an exact machine, enabled project, provider and model. Accepted sessions keep their original selection when a profile is edited.</p><ul class="space-y-3">{rows}</ul>{next}{editor}</section>"#,
        editor = editor(view)
    )
}

fn editor(view: &Page<'_>) -> String {
    let (Some(machine), Some(runtime)) = (view.machine, view.runtime) else {
        return "<p>Select a machine to configure a profile.</p>".into();
    };
    let Some(capabilities) = &machine.capabilities else {
        return String::new();
    };
    let base = base(view.company.id);
    let seeded = view
        .profile
        .filter(|profile| profile.selection.machine_id == machine.machine.id);
    let preferred = view
        .provider
        .or_else(|| seeded.map(|profile| profile.selection.provider));
    let provider = capabilities
        .catalog
        .providers
        .iter()
        .find(|provider| Some(provider.provider) == preferred)
        .or_else(|| capabilities.catalog.providers.first());
    let Some(provider) = provider else {
        return "<p>No native provider is advertised. Install and authenticate the CLI locally, then refresh the catalog.</p>".into();
    };
    let preferred_model = view
        .model
        .or_else(|| seeded.map(|profile| &profile.selection.model));
    let model = provider
        .models
        .iter()
        .find(|model| Some(&model.model) == preferred_model)
        .or_else(|| provider.models.first());
    let Some(model) = model else {
        return "<p>The selected provider advertises no models.</p>".into();
    };
    let providers = capabilities
        .catalog
        .providers
        .iter()
        .map(|entry| {
            option(
                &enum_value(&entry.provider),
                &enum_value(&entry.provider),
                entry.provider == provider.provider,
            )
        })
        .collect::<String>();
    let models = provider
        .models
        .iter()
        .map(|entry| {
            option(
                entry.model.as_str(),
                entry.model.as_str(),
                entry.model == model.model,
            )
        })
        .collect::<String>();
    let profile_hidden = view.profile.map_or_else(String::new, |profile| {
        format!(
            r#"<input type="hidden" name="profile" value="{}">"#,
            profile.id
        )
    });
    let picker = format!(
        r#"<form method="get" action="{base}" class="space-y-3"><input type="hidden" name="machine" value="{machine}">{profile_hidden}{context}<label class="block">Provider<select class="select w-full" name="provider">{providers}</select></label><label class="block">Model<select class="select w-full" name="model">{models}</select></label><button type="submit" class="btn btn-ghost">Choose provider / model</button></form>"#,
        machine = machine.machine.id,
        context = conversation_fields(view)
    );
    let projects = runtime
        .projects
        .iter()
        .map(|project| {
            option(
                project.as_str(),
                project.as_str(),
                seeded.is_some_and(|profile| &profile.selection.project_id == project),
            )
        })
        .collect::<String>();
    let reasoning = model
        .reasoning_levels
        .iter()
        .map(|level| {
            option(
                &enum_value(level),
                &enum_value(level),
                seeded.is_some_and(|profile| profile.selection.policy.reasoning == *level),
            )
        })
        .collect::<String>();
    let fields = format!(
        r#"<input type="hidden" name="machine_id" value="{machine}"><input type="hidden" name="provider" value="{provider}"><input type="hidden" name="model" value="{model}"><label class="block">Profile name<input class="input w-full" name="name" maxlength="128" required value="{name}"></label><label class="block">Enabled project<select class="select w-full" name="project" required>{projects}</select></label><label class="block">Reasoning<select class="select w-full" name="reasoning" required>{reasoning}</select></label><p>Workspace: existing project directory. Permissions: provider defaults. Native prompts are answered in Cloudroom.</p>"#,
        machine = machine.machine.id,
        provider = attr(&enum_value(&provider.provider)),
        model = attr(model.model.as_str()),
        name = attr(view.profile.map_or("", |profile| profile.name.as_str()))
    );
    let action = view.profile.map_or_else(
        || format!("{base}/profiles"),
        |profile| format!("{base}/profiles/{}", profile.id),
    );
    format!(
        "<article class=\"card card-border p-4 space-y-4\"><h3 class=\"font-semibold\">{title}</h3>{picker}{editor}</article>",
        title = if view.profile.is_some() {
            "Edit profile"
        } else {
            "New profile"
        },
        editor = form(&action, &fields, "Save profile")
    )
}
