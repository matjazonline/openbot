//! What a `/ui` region shows while the content that will replace it is still in flight.
//!
//! Every workspace is built the same way — a sidebar list and a detail pane, swapped over htmx —
//! so the placeholders are defined once here by *shape* rather than per page. A swap target opts
//! in by carrying [`Skeleton::attribute`]; [`skeleton_script`] paints the matching shape into it
//! for the length of the request and puts the region back if no swap ever comes.
//!
//! The shapes are daisyUI `skeleton` blocks laid out like the thing they stand in for, so the
//! screen does not jump when the real content lands.

/// Declares the placeholder shapes in one place: the enum the script's lookup table is built
/// from, and the attribute constant each page writes into its markup.
///
/// The two are generated from the same name, so a page can never opt into a shape the table does
/// not hold -- which would render as a region that simply never shows it is loading.
macro_rules! skeleton_shapes {
    ($($(#[$doc:meta])* $variant:ident => $name:literal, $attribute:ident;)*) => {
        /// The placeholder a swap target shows while it is being replaced.
        ///
        /// One variant per *shape* on screen, not per page: the Agents, Channels, Companies, Team,
        /// Tasks and Outbox panes are the same rectangle in the same place, and a reader should not
        /// be able to tell which workspace they are waiting in from the way it loads.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum Skeleton {
            $($(#[$doc])* $variant,)*
        }

        impl Skeleton {
            const ALL: &'static [Skeleton] = &[$(Skeleton::$variant,)*];

            /// The value `data-skeleton` carries, and the key the script looks the shape up by.
            fn name(self) -> &'static str {
                match self {
                    $(Skeleton::$variant => $name,)*
                }
            }
        }

        $(
            $(#[$doc])*
            ///
            /// Written into a swap target's tag, next to its `id`.
            pub(crate) const $attribute: &str = concat!(r#" data-skeleton=""#, $name, r#"""#);
        )*
    };
}

skeleton_shapes! {
    /// A right-hand detail pane: a titled header over a body of fields.
    Pane => "pane", PANE_SKELETON;
    /// A sidebar list: a summary line over rows of badge-and-two-lines.
    List => "list", LIST_SKELETON;
    /// The mailbox's middle column: a channel header over thread rows.
    ThreadColumn => "thread-column", THREAD_COLUMN_SKELETON;
    /// One further page of thread rows, appended below the rows already on screen.
    ThreadRows => "thread-rows", THREAD_ROWS_SKELETON;
    /// The dashboard's stack of panels.
    Panels => "panels", PANELS_SKELETON;
}

impl Skeleton {
    /// Whether the placeholder is added *below* what is already there rather than replacing it.
    ///
    /// Paging a list keeps the rows the reader is looking at; everything else is a replacement.
    fn appends(self) -> bool {
        matches!(self, Skeleton::ThreadRows)
    }

    /// The classes the target wears while the placeholder is up, when its own do not fit it.
    ///
    /// A detail pane is centred while it holds a one-line "pick something" message and a column
    /// once it holds content; the placeholder is always a column, so it says so rather than
    /// depending on which of the two it replaced.
    fn class(self) -> Option<&'static str> {
        match self {
            Skeleton::Pane => Some("flex flex-1 flex-col gap-4 overflow-hidden bg-base-100 p-6"),
            _ => None,
        }
    }

    fn markup(self) -> String {
        match self {
            Skeleton::Pane => pane_markup(),
            Skeleton::List => list_markup(),
            Skeleton::ThreadColumn => thread_column_markup(),
            Skeleton::ThreadRows => thread_rows_markup(),
            Skeleton::Panels => panels_markup(),
        }
    }
}

/// Renders `body` `count` times, for the rows a placeholder is mostly made of.
fn repeat(count: usize, body: &str) -> String {
    body.repeat(count)
}

fn pane_markup() -> String {
    format!(
        r##"<div class="flex items-center gap-3">
    <div class="skeleton h-12 w-12 shrink-0 rounded-full"></div>
    <div class="flex min-w-0 flex-1 flex-col gap-2">
        <div class="skeleton h-5 w-2/5"></div>
        <div class="skeleton h-3 w-1/4"></div>
    </div>
</div>
<div class="skeleton h-12 w-full"></div>
<div class="flex flex-col gap-2 rounded-box border border-base-300 p-4">{rows}</div>
<div class="skeleton h-40 w-full"></div>"##,
        rows = repeat(
            5,
            r##"<div class="flex items-center justify-between gap-4">
        <div class="skeleton h-3 w-24"></div>
        <div class="skeleton h-3 w-1/3"></div>
    </div>"##,
        ),
    )
}

fn list_markup() -> String {
    format!(
        r##"<div class="flex items-center justify-between gap-2 px-3 py-2">
    <div class="skeleton h-3 w-28"></div>
    <div class="skeleton h-4 w-4 rounded-full"></div>
</div>
<div class="flex flex-col gap-1 px-2">{rows}</div>"##,
        rows = repeat(7, &list_row_markup()),
    )
}

fn list_row_markup() -> String {
    r##"<div class="flex flex-col gap-2 rounded-box px-3 py-2">
        <div class="flex items-center gap-2">
            <div class="skeleton h-4 w-14 shrink-0 rounded-full"></div>
            <div class="skeleton h-3 flex-1"></div>
        </div>
        <div class="skeleton h-2.5 w-3/4"></div>
    </div>"##
        .to_string()
}

fn thread_column_markup() -> String {
    format!(
        r##"<div class="flex items-center justify-between gap-2 border-b border-base-300 px-4 py-3">
    <div class="flex min-w-0 flex-col gap-2">
        <div class="skeleton h-5 w-32"></div>
        <div class="skeleton h-3 w-40"></div>
    </div>
    <div class="skeleton h-8 w-28 shrink-0"></div>
</div>
<div class="flex flex-col gap-1 overflow-hidden p-2">{rows}</div>"##,
        rows = repeat(8, &thread_row_markup()),
    )
}

fn thread_rows_markup() -> String {
    format!(
        r##"<div class="flex flex-col gap-1 p-2">{rows}</div>"##,
        rows = repeat(3, &thread_row_markup()),
    )
}

fn thread_row_markup() -> String {
    r##"<div class="flex flex-col gap-2 px-3 py-3">
        <div class="flex items-center gap-2">
            <div class="skeleton h-3 flex-1"></div>
            <div class="skeleton h-2.5 w-12 shrink-0"></div>
        </div>
        <div class="skeleton h-2.5 w-2/3"></div>
    </div>"##
        .to_string()
}

/// The dashboard's shape: two headed groups of panels, then the process strip.
fn panels_markup() -> String {
    let group = |panels: usize| {
        format!(
            r##"<div class="flex flex-col gap-3">
        <div class="skeleton h-5 w-48"></div>
        {panels}
    </div>"##,
            panels = repeat(panels, r##"<div class="skeleton h-28 w-full"></div>"##),
        )
    };

    format!(
        r##"<div class="flex flex-col gap-6 p-6">
    {queues}
    {window}
    <div class="skeleton h-20 w-full"></div>
</div>"##,
        queues = group(3),
        window = group(2),
    )
}

/// The dashboard's placeholder, for the page to render in place of panels it has not fetched yet.
///
/// The same markup the script paints, so the shape the reader sees while the shell loads and the
/// shape they see on a later refresh are the one definition.
pub(crate) fn panels_placeholder() -> String {
    Skeleton::Panels.markup()
}

/// The lookup table and the behaviour around it, as one block of JavaScript for the `/ui` shell.
///
/// The markup is built in Rust so the placeholders live beside the pages they stand in for, and is
/// handed to the script as JSON string literals rather than assembled in the browser.
pub(crate) fn skeleton_script() -> String {
    let shapes: String = Skeleton::ALL
        .iter()
        .map(|shape| {
            format!(
                "            {name}: {{ markup: {markup}, className: {class}, appends: {appends} }},\n",
                name = js_string(shape.name()),
                markup = js_string(&shape.markup()),
                class = match shape.class() {
                    Some(class) => js_string(class),
                    None => "null".to_string(),
                },
                appends = shape.appends(),
            )
        })
        .collect();

    format!(
        r##"        var UI_SKELETONS = {{
{shapes}        }};
{SKELETON_BEHAVIOUR}"##
    )
}

/// One JavaScript string literal, safe to sit inside a `<script>` element.
///
/// `serde_json` gives the quoting and the escapes; the `</` rewrite is what stops any markup that
/// ever contains a closing tag from ending the script element early.
fn js_string(value: &str) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "\"\"".to_string())
        .replace("</", "<\\/")
}

/// Puts a placeholder up for the length of a request, and takes it down again.
///
/// Painting on `htmx:beforeRequest` rather than on the click means it covers every way a region is
/// replaced — a menu entry, a filter change, a pager, a submitted form — without any of them
/// having to ask for it.
const SKELETON_BEHAVIOUR: &str = r##"
        (function () {
            // Keyed by element, so a region swapped away takes its saved content with it.
            var painted = new WeakMap();

            function paint(target) {
                var shape = UI_SKELETONS[target.dataset.skeleton];
                if (!shape || painted.has(target)) return;
                target.setAttribute('aria-busy', 'true');

                if (shape.appends) {
                    var addition = document.createElement('div');
                    addition.innerHTML = shape.markup;
                    target.appendChild(addition);
                    painted.set(target, { addition: addition });
                    return;
                }

                // The nodes themselves are kept, not their markup: the element that triggered
                // the request is usually one of them, and htmx reads the root node of that
                // element to find where an `hx-swap-oob` fragment belongs. Re-created from a
                // string, the trigger is left detached for good and every out-of-band fragment
                // in the response is silently dropped -- which is what kept a newly created
                // company out of the sidebar beside it.
                var kept = document.createDocumentFragment();
                while (target.firstChild) {
                    kept.appendChild(target.firstChild);
                }
                painted.set(target, { nodes: kept, className: target.className });
                if (shape.className) {
                    target.className = shape.className;
                }
                target.innerHTML = shape.markup;
            }

            function restore(target) {
                var previous = painted.get(target);
                if (!previous) return;
                painted.delete(target);
                target.removeAttribute('aria-busy');
                if (previous.addition) {
                    previous.addition.remove();
                    return;
                }
                target.className = previous.className;
                target.replaceChildren(previous.nodes);
            }

            document.body.addEventListener('htmx:beforeRequest', function (event) {
                var target = event.detail.target;
                if (target && target.dataset && target.dataset.skeleton) {
                    paint(target);
                }
            });

            // Undoing the placeholder *before* htmx decides what to do with the response costs
            // nothing when a swap follows -- it overwrites the region in the same frame -- and is
            // what leaves the reader's own content behind when one does not, as after an error.
            document.body.addEventListener('htmx:beforeSwap', function (event) {
                if (event.detail.target) {
                    restore(event.detail.target);
                }
            });

            // A request that never reached the server gets no swap at all, so this is the only
            // place a placeholder put up for one comes down.
            document.body.addEventListener('htmx:afterRequest', function (event) {
                if (event.detail.target) {
                    restore(event.detail.target);
                }
            });
        })();"##;

#[cfg(test)]
mod tests {
    use super::*;

    /// The attribute a page writes and the key the script looks up are the same string, or the
    /// region simply never shows a placeholder -- a failure nothing else would notice.
    #[test]
    fn every_attribute_a_page_can_write_is_in_the_table() {
        let script = skeleton_script();

        for attribute in [
            PANE_SKELETON,
            LIST_SKELETON,
            THREAD_COLUMN_SKELETON,
            THREAD_ROWS_SKELETON,
            PANELS_SKELETON,
        ] {
            let name = attribute
                .split('"')
                .nth(1)
                .expect("the attribute quotes its value");
            assert!(
                Skeleton::ALL.iter().any(|shape| shape.name() == name),
                "{name} names no shape"
            );
            assert!(
                script.contains(&format!("            {name:?}: {{ markup:")),
                "{name} is missing from the lookup table: {script}"
            );
        }
    }

    /// Every placeholder is made of daisyUI `skeleton` blocks; one built out of plain divs would
    /// render as an invisible gap rather than as something loading.
    #[test]
    fn every_shape_is_built_from_daisyui_skeletons() {
        for &shape in Skeleton::ALL {
            assert!(
                shape.markup().contains(r#"class="skeleton "#),
                "{shape:?} has no skeleton blocks in it"
            );
        }
    }

    /// The script is inlined into a `<script>` element, so no string in it may close that element.
    #[test]
    fn the_table_cannot_end_the_script_element() {
        assert!(!skeleton_script().contains("</"));
    }
}
