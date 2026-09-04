#!/usr/bin/env bash
#
# The abstraction gate. It replaces the structural limitation the email-shaped spine used to
# provide: while every canonical message was an email row, a transport type could not leak into
# the application layer without the compiler saying so. The canonical spine is transport-neutral
# now, so nothing in the type system objects when a producer reaches for an address, a header or
# an adapter -- this script is what objects instead, before Slack code lands beside it.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

failures="$(mktemp)"
trap 'rm -f "$failures"' EXIT

# Everything under `src/adapters/protocols/email`, for the rules that name the email adapter as the
# one place allowed to break them.
email_adapter='!src/adapters/protocols/email/**'

# A file's production code: an inline `#[cfg(test)] mod tests` is dropped, along with everything
# after it. A test may build the values a transport hands us -- an inbound RFC id, an address a
# customer wrote -- because simulating a provider is what a test is for. What these rules forbid is
# *production* code inventing them.
production_source() {
    awk '
        pending_cfg_test {
            if ($0 ~ /^[[:space:]]*mod[[:space:]]/) exit
            print "#[cfg(test)]"
            pending_cfg_test = 0
        }
        /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*$/ {
            pending_cfg_test = 1
            next
        }
        { print }
    ' "$1"
}

# Run one pattern over the production code of every matching file, reporting the file each hit came
# from -- `rg` reads one file at a time here, so it cannot name it for us.
scan_production() {
    local description="$1" pattern="$2"
    shift 2
    local file
    while IFS= read -r file; do
        case "$file" in
            *_tests.rs|*/tests.rs|*/test_support.rs) continue ;;
        esac
        if production_source "$file" | rg -n --pcre2 "$pattern" >>"$failures"; then
            printf '%s in %s\n' "$description" "$file" >>"$failures"
        fi
    # Caller globs go last: ripgrep resolves overlapping globs by the last one that matches, so an
    # exclusion placed before `*.rs` would be undone by it.
    done < <(rg --files -g '*.rs' "$@")
}

# The application and the domain coordinate and decide; they never import the things that talk to a
# provider or a database. An allowlist belongs here rather than in a reviewer's memory -- keep it
# empty for as long as it can be kept empty.
scan_production 'application boundary violation' \
    '(^|[^[:alnum:]_])(crate::)?adapters::|\b(sqlx|axum|lettre|mail_parser|slack_morphism)::' \
    src/application
scan_production 'domain boundary violation' \
    '\b(sqlx|axum|lettre|mail_parser|slack_morphism)::|crate::adapters::' \
    src/domain

# The canonical spine is what every transport shares. A provider's vocabulary on one of these three
# entities is the seam this whole refactor removed: it makes the second transport a branch inside
# the first one's shape rather than a peer beside it.
if rg -n '\b(EmailAddress|EmailMessageMetadata|MessageId|EmailIdentity|SlackTeamId|SlackUserId|SlackConversationId|SlackTimestamp)\b' \
    src/domain/entities/message.rs src/domain/entities/thread.rs src/domain/entities/participant.rs \
    >>"$failures"; then
    printf 'provider type leaked into a canonical message/thread/participant entity\n' \
        >>"$failures"
fi

# `email_messages` was the canonical message store and `email_outbox` the delivery queue. They are
# `messages` and `message_deliveries` now; a query naming either is reading a schema that no
# migration creates.
if rg -n -i '\b(CREATE[[:space:]]+TABLE|ALTER[[:space:]]+TABLE|FROM|JOIN|INTO|UPDATE|REFERENCES)[[:space:]]+(email_messages|email_outbox)\b' \
    migrations src -g '*.sql' -g '*.rs' >>"$failures"; then
    printf 'retired email-shaped SQL table referenced\n' >>"$failures"
fi

# The types that looked transport-neutral and were not. `NormalizedInboundMessage` in particular
# was turned straight back into a `ParsedEmail` by the use case that received it.
if rg -n '\b(NormalizedInboundMessage|NormalizedOutboundMessage|ParticipantIdentity|ChannelType|OutboxEmail|OutboundSend)\b|parsed_email_from_normalized' \
    src >>"$failures"; then
    printf 'retired compatibility type referenced\n' >>"$failures"
fi

# A `.invalid` address is how a participant with no mailbox used to be given one anyway, so that
# an email-keyed table would accept it. Nothing outside the email adapter has a reason to write one.
if rg -n '@[^[:space:]<>]*\.invalid\b|<[^>[:space:]]*\.invalid>' \
    src -g '*.rs' -g "$email_adapter" >>"$failures"; then
    printf 'synthetic email identity escaped the email adapter\n' >>"$failures"
fi

# An RFC `Message-ID` is `<local@domain>`, and only the email adapter decides what one looks like.
# A producer that builds its own puts a value no mail was ever sent under into a live
# `In-Reply-To:` -- it threads onto nothing in a recipient's client, and no reply quoting it can be
# resolved back here. Producers say what they are answering with `EmailThreading` instead, and the
# renderer derives the header from that.
scan_production 'an RFC Message-ID was constructed outside the email adapter' \
    '"<[^">]*@' src -g "$email_adapter"

# The frozen mail itself. Constructing one outside the renderer is deciding what goes on an
# envelope somewhere the retry-stability rules do not apply.
if rg -n '\bOutboundEmail(V[0-9]+)?[[:space:]]*\{' \
    src -g '*.rs' -g "$email_adapter" \
    | rg -v -- '->[[:space:]]*.*OutboundEmail' >>"$failures"; then
    printf 'outbound email constructed outside the email adapter\n' >>"$failures"
fi

if [[ -s "$failures" ]]; then
    printf 'transport boundary check failed:\n' >&2
    cat "$failures" >&2
    exit 1
fi

printf 'transport boundary check passed\n'
