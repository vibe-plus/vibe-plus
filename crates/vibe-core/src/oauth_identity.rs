//! Enrich `Credential` JSON with non-persistent fields parsed from stored OAuth JWTs.

use vibe_protocol::Credential;

use crate::auth_fingerprint::chatgpt_oauth_hints_from_access_token;

pub(crate) fn credential_attach_oauth_identity(c: &mut Credential) {
    let persisted_email = c.oauth_account_email.clone();
    let persisted_subject = c.oauth_account_subject.clone();
    let persisted_plan = c.oauth_chatgpt_plan_slug.clone();

    let Some(tok) = c.oauth_access_token.as_deref().filter(|t| !t.is_empty()) else {
        c.oauth_account_email = persisted_email;
        c.oauth_account_subject = persisted_subject;
        c.oauth_chatgpt_plan_slug = persisted_plan;
        return;
    };
    // API-key style secrets are not JWTs — skip decode noise; keep DB-cached identity from id_token import.
    if tok.starts_with("sk-") {
        c.oauth_account_email = None;
        c.oauth_account_subject = None;
        c.oauth_chatgpt_plan_slug = None;
        return;
    }
    let hints = chatgpt_oauth_hints_from_access_token(tok);
    c.oauth_account_email = hints.email.or(persisted_email);
    c.oauth_account_subject = hints
        .subject
        .clone()
        .or(hints.chatgpt_user_id.clone())
        .or(persisted_subject);
    c.oauth_chatgpt_plan_slug = hints.chatgpt_plan_slug.or(persisted_plan);
}

pub(crate) fn credentials_attach_oauth_identities(rows: &mut [Credential]) {
    for c in rows.iter_mut() {
        credential_attach_oauth_identity(c);
    }
}
