// SPDX-License-Identifier: MIT

//! Process/composition coverage for the non-demo harness-owner facade.

use context_service::{ControlCapabilities, GrantRegistry};
use context_service::{
    ControlCommand, ControlDraft as Draft, ControlEligibleItem as EligibleItem, ControlItemRef,
    ControlOperation, ControlPatch, ControlPlane, ControlPreview as Preview,
    ControlReceipt as Receipt, ControlRevision as Revision, ControlScope, ControlState,
    FacadePermission, FacadeRequest, HarnessBackedContextService, HarnessFacadeConfig,
    HarnessOwnerClient, HarnessOwnerPort, OwnerAuthorization, OwnerError, PreviewRequest,
    ProtectedAuthReference, SecretDigest,
};
use std::collections::BTreeSet;

const HOST: &str = "console.test";
const ORIGIN: &str = "https://console.test";
const CSRF: &[u8] = b"request-csrf";
const FULL_TOKEN: &[u8] = b"full-capability";
const METADATA_TOKEN: &[u8] = b"metadata-only";
const SHORT_TOKEN: &[u8] = b"short-lived";
const EDIT_TOKEN: &[u8] = b"edit-only";

#[derive(Debug)]
struct RecordingOwner {
    inner: ControlPlane,
    calls: Vec<String>,
    commit_effects: BTreeSet<String>,
}

impl RecordingOwner {
    fn new(inner: ControlPlane) -> Self {
        Self {
            inner,
            calls: Vec::new(),
            commit_effects: BTreeSet::new(),
        }
    }

    fn record(&mut self, operation: &str) {
        self.calls.push(operation.to_owned());
    }

    fn scope(&self) -> ControlScope {
        self.inner.scope().clone()
    }
}

impl HarnessOwnerPort for RecordingOwner {
    fn capabilities(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
    ) -> Result<ControlCapabilities, OwnerError> {
        self.record("capabilities");
        HarnessOwnerPort::capabilities(&mut self.inner, auth, scope)
    }

    fn state(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
    ) -> Result<ControlState, OwnerError> {
        self.record("state");
        HarnessOwnerPort::state(&mut self.inner, auth, scope)
    }

    fn eligible_items(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
    ) -> Result<Vec<EligibleItem>, OwnerError> {
        self.record("eligible_items");
        HarnessOwnerPort::eligible_items(&mut self.inner, auth, scope)
    }

    fn revisions(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
    ) -> Result<Vec<Revision>, OwnerError> {
        self.record("revisions");
        HarnessOwnerPort::revisions(&mut self.inner, auth, scope)
    }

    fn drafts(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
    ) -> Result<Vec<Draft>, OwnerError> {
        self.record("drafts");
        HarnessOwnerPort::drafts(&mut self.inner, auth, scope)
    }

    fn previews(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
    ) -> Result<Vec<Preview>, OwnerError> {
        self.record("previews");
        HarnessOwnerPort::previews(&mut self.inner, auth, scope)
    }

    fn receipts(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
    ) -> Result<Vec<Receipt>, OwnerError> {
        self.record("receipts");
        HarnessOwnerPort::receipts(&mut self.inner, auth, scope)
    }

    fn get_draft(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
        draft_id: &str,
    ) -> Result<Draft, OwnerError> {
        self.record("get_draft");
        HarnessOwnerPort::get_draft(&mut self.inner, auth, scope, draft_id)
    }

    fn get_preview(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
        preview_id: &str,
    ) -> Result<Preview, OwnerError> {
        self.record("get_preview");
        HarnessOwnerPort::get_preview(&mut self.inner, auth, scope, preview_id)
    }

    fn get_receipt(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
        command_id: &str,
    ) -> Result<Receipt, OwnerError> {
        self.record("get_receipt");
        HarnessOwnerPort::get_receipt(&mut self.inner, auth, scope, command_id)
    }

    fn create_draft(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: ControlScope,
        expected_active_revision_id: &str,
        author_ref: &str,
    ) -> Result<Draft, OwnerError> {
        self.record("create_draft");
        HarnessOwnerPort::create_draft(
            &mut self.inner,
            auth,
            scope,
            expected_active_revision_id,
            author_ref,
        )
    }

    fn apply_patch(
        &mut self,
        auth: &ProtectedAuthReference,
        patch: ControlPatch,
        author_ref: &str,
        authorization: OwnerAuthorization,
    ) -> Result<Draft, OwnerError> {
        self.record("apply_patch");
        HarnessOwnerPort::apply_patch(&mut self.inner, auth, patch, author_ref, authorization)
    }

    #[allow(clippy::too_many_arguments)]
    fn create_preview(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: ControlScope,
        draft_id: &str,
        expected_draft_version: u64,
        applicable_requested: bool,
        expected_control_version: u64,
        risk_ack: bool,
    ) -> Result<Preview, OwnerError> {
        self.record("create_preview");
        HarnessOwnerPort::create_preview(
            &mut self.inner,
            auth,
            scope,
            draft_id,
            expected_draft_version,
            applicable_requested,
            expected_control_version,
            risk_ack,
        )
    }

    fn pause(
        &mut self,
        auth: &ProtectedAuthReference,
        command: ControlCommand,
    ) -> Result<Receipt, OwnerError> {
        self.record("pause");
        HarnessOwnerPort::pause(&mut self.inner, auth, command)
    }

    fn commit(
        &mut self,
        auth: &ProtectedAuthReference,
        command: ControlCommand,
    ) -> Result<Receipt, OwnerError> {
        self.record("commit");
        let receipt = HarnessOwnerPort::commit(&mut self.inner, auth, command)?;
        self.commit_effects.insert(receipt.command_id.clone());
        Ok(receipt)
    }

    fn resume(
        &mut self,
        auth: &ProtectedAuthReference,
        command: ControlCommand,
    ) -> Result<Receipt, OwnerError> {
        self.record("resume");
        HarnessOwnerPort::resume(&mut self.inner, auth, command)
    }
}

fn grants(scope: &ControlScope, token: &[u8], expiry: u64) -> GrantRegistry {
    let mut grants = GrantRegistry::new();
    for (id, permission) in [
        ("metadata", FacadePermission::MetadataRead),
        ("content", FacadePermission::ContentRead),
        ("edit", FacadePermission::Edit),
        ("objective", FacadePermission::Objective),
        ("commit", FacadePermission::Commit),
        ("pause", FacadePermission::Pause),
        ("resume", FacadePermission::Resume),
    ] {
        grants
            .issue(id, permission, scope.clone(), token, expiry)
            .expect("grant");
    }
    grants
}

fn edit_grant(scope: &ControlScope, token: &[u8], expiry: u64) -> GrantRegistry {
    let mut grants = GrantRegistry::new();
    grants
        .issue(
            "edit-only",
            FacadePermission::Edit,
            scope.clone(),
            token,
            expiry,
        )
        .expect("edit grant");
    grants
}

fn request(principal: &str, token: &[u8], now: u64) -> FacadeRequest {
    FacadeRequest::new(
        principal,
        token,
        HOST,
        Some(ORIGIN.to_owned()),
        Some(CSRF),
        now,
    )
}

fn config(scope: ControlScope) -> HarnessFacadeConfig {
    HarnessFacadeConfig::new(
        scope,
        HOST,
        Some(ORIGIN.to_owned()),
        Some(SecretDigest::from_secret(CSRF).expect("csrf digest")),
        Default::default(),
    )
    .expect("config")
}

fn command(
    plane: &ControlPlane,
    kind: &str,
    key: &str,
    expected_control_version: u64,
) -> ControlCommand {
    let state = plane.state();
    ControlCommand {
        schema: "ascension.context-control.command.v1".to_owned(),
        scope: plane.scope().clone(),
        idempotency_key: key.to_owned(),
        command_window_id: state.command_window_id,
        expected_control_version,
        kind: kind.to_owned(),
        expected_active_revision_id: None,
        preview_id: None,
        approved_manifest_sha256: None,
        expected_preview_id: None,
    }
}

fn history(plane: &ControlPlane) -> ControlItemRef {
    plane
        .eligible_items()
        .into_iter()
        .find(|item| item.item.item_id == "history-1")
        .expect("history")
        .item
}

#[test]
fn harness_owner_composition_delegates_scoped_held_flow_and_redacts_metadata() {
    let owner = RecordingOwner::new(ControlPlane::synthetic());
    let scope = owner.scope();
    let auth = ProtectedAuthReference::new("owner-key-ref").expect("auth ref");
    let mut service = HarnessBackedContextService::new(
        HarnessOwnerClient::new(owner, auth),
        grants(&scope, FULL_TOKEN, 10_000),
        config(scope.clone()),
    )
    .expect("service");
    let full = request("studio-operator", FULL_TOKEN, 100);
    let metadata_items = service.eligible_items(&full, false).expect("metadata");
    assert!(
        metadata_items.iter().all(|item| item.content.is_none()),
        "metadata callers never receive raw content"
    );
    assert!(
        service
            .eligible_items(&full, true)
            .expect("content")
            .iter()
            .any(|item| item.content.is_some())
    );

    let active = service.state(&full).expect("state").active_revision_id;
    let draft = service.create_draft(&full, &active).expect("draft");
    let item = history(&service.owner().inner);
    let edited = service
        .edit_draft(
            &full,
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope: scope.clone(),
                draft_id: draft.draft_id.clone(),
                expected_draft_version: draft.version,
                expected_active_revision_id: active.clone(),
                operations: vec![
                    ControlOperation::IncludeItem { item: item.clone() },
                    ControlOperation::PinItem { item: item.clone() },
                ],
            },
        )
        .expect("edit");
    assert_eq!(edited.pinned_item_ids, vec![item.item_id.clone()]);

    let owner_state = service.owner().inner.state();
    let pause_command = command(
        &service.owner().inner,
        "pause",
        "pause-composed",
        owner_state.control_version,
    );
    let pause_receipt = service.pause(&full, pause_command).expect("pause");
    assert_eq!(pause_receipt.kind, "pause");

    let preview = service
        .preview(
            &full,
            PreviewRequest {
                scope: scope.clone(),
                draft_id: edited.draft_id.clone(),
                expected_draft_version: edited.version,
                applicable_requested: true,
                expected_control_version: service.owner().inner.state().control_version,
                unknown_total_risk_acknowledged: true,
            },
        )
        .expect("held preview");
    assert!(preview.applicable);
    assert!(
        preview
            .components
            .iter()
            .all(|component| !component.content_ref.starts_with("http"))
    );

    let mut commit_command = command(
        &service.owner().inner,
        "commit",
        "commit-composed",
        service.owner().inner.state().control_version,
    );
    commit_command.expected_active_revision_id = Some(active);
    commit_command.preview_id = Some(preview.preview_id.clone());
    commit_command.approved_manifest_sha256 = preview.prepared_manifest_sha256.clone();
    let committed = service
        .held_commit(&full, commit_command.clone())
        .expect("commit");

    let mut resume_command = command(
        &service.owner().inner,
        "resume",
        "resume-composed",
        service.owner().inner.state().control_version,
    );
    resume_command.expected_active_revision_id = Some(committed.active_revision_id.clone());
    resume_command.expected_preview_id = Some(preview.preview_id);
    let resumed = service.resume(&full, resume_command).expect("resume");
    assert_eq!(resumed.kind, "resume");

    let receipt = service
        .read_receipt(&full, &committed.command_id)
        .expect("receipt");
    assert_eq!(receipt, committed);
    assert!(service.owner().calls.contains(&"apply_patch".to_owned()));
    assert!(service.owner().calls.contains(&"create_preview".to_owned()));
    assert!(service.owner().calls.contains(&"commit".to_owned()));
    assert!(service.owner().calls.contains(&"resume".to_owned()));
    assert_eq!(service.owner().commit_effects.len(), 1);

    let journal = service
        .owner()
        .inner
        .export_journal()
        .expect("durable owner journal");
    let recovered_owner = RecordingOwner::new(
        ControlPlane::recover_journal(&journal).expect("recover owner journal"),
    );
    let recovered_scope = recovered_owner.scope();
    let mut recovered = HarnessBackedContextService::new(
        HarnessOwnerClient::new(
            recovered_owner,
            ProtectedAuthReference::new("owner-key-ref").expect("auth ref"),
        ),
        grants(&recovered_scope, FULL_TOKEN, 10_000),
        config(recovered_scope),
    )
    .expect("recovered service");
    assert_eq!(
        recovered
            .read_receipt(&full, &committed.command_id)
            .expect("receipt after restart"),
        committed
    );
    let replayed = recovered
        .held_commit(&full, commit_command)
        .expect("idempotent commit replay");
    assert_eq!(replayed, committed);
    assert_eq!(recovered.owner().commit_effects.len(), 1);
}

#[test]
fn grants_and_request_proofs_are_independent_and_fail_before_mutation() {
    let owner = RecordingOwner::new(ControlPlane::synthetic());
    let scope = owner.scope();
    let auth = ProtectedAuthReference::new("owner-key-ref").expect("auth ref");
    let mut service_grants = grants(&scope, FULL_TOKEN, 10_000);
    service_grants
        .issue(
            "metadata-only",
            FacadePermission::MetadataRead,
            scope.clone(),
            METADATA_TOKEN,
            10_000,
        )
        .expect("metadata grant");
    let mut service = HarnessBackedContextService::new(
        HarnessOwnerClient::new(owner, auth),
        service_grants,
        config(scope.clone()),
    )
    .expect("service");

    let metadata_only = request("metadata-reader", METADATA_TOKEN, 100);
    let before = service.owner().inner.state();
    let denied = service
        .create_draft(&metadata_only, &before.active_revision_id)
        .expect_err("metadata grant cannot mutate");
    assert_eq!(denied.code, "edit_grant_required");
    assert_eq!(service.owner().inner.state(), before);

    let full = request("editor", FULL_TOKEN, 100);
    let _draft = service
        .create_draft(&full, &before.active_revision_id)
        .expect("draft");
    let ordinary_owner = RecordingOwner::new(ControlPlane::synthetic());
    let ordinary_scope = ordinary_owner.scope();
    let mut ordinary_service = HarnessBackedContextService::new(
        HarnessOwnerClient::new(
            ordinary_owner,
            ProtectedAuthReference::new("owner-key-ref").expect("auth ref"),
        ),
        edit_grant(&ordinary_scope, EDIT_TOKEN, 10_000),
        config(ordinary_scope.clone()),
    )
    .expect("ordinary edit service");
    let ordinary = request("editor", EDIT_TOKEN, 100);
    let ordinary_revision = "revision-1".to_owned();
    let ordinary_draft = ordinary_service
        .create_draft(&ordinary, &ordinary_revision)
        .expect("ordinary draft");
    let objective_patch = ControlPatch {
        schema: "ascension.context-control.patch.v1".to_owned(),
        scope: ordinary_scope.clone(),
        draft_id: ordinary_draft.draft_id.clone(),
        expected_draft_version: ordinary_draft.version,
        expected_active_revision_id: ordinary_revision,
        operations: vec![ControlOperation::SetObjective {
            text: "new objective".to_owned(),
        }],
    };
    let objective_denied = ordinary_service
        .edit_draft(&ordinary, objective_patch)
        .expect_err("ordinary edit cannot replace objective");
    assert_eq!(objective_denied.code, "objective_grant_required");
    let resume_denied = ordinary_service
        .resume(
            &ordinary,
            command(
                &ordinary_service.owner().inner,
                "resume",
                "ordinary-resume",
                ordinary_service.owner().inner.state().control_version,
            ),
        )
        .expect_err("ordinary edit cannot resume");
    assert_eq!(resume_denied.code, "resume_grant_required");

    let forged = FacadeRequest::new(
        "editor",
        FULL_TOKEN,
        "attacker.test",
        Some(ORIGIN.to_owned()),
        Some(CSRF),
        100,
    );
    let host_denied = service
        .create_draft(&forged, &before.active_revision_id)
        .expect_err("forged host");
    assert_eq!(host_denied.code, "host_not_allowed");

    let mut revoked_grants = grants(&scope, FULL_TOKEN, 10_000);
    revoked_grants.revoke("edit").expect("revoke");
    let revoked_owner = RecordingOwner::new(ControlPlane::synthetic());
    let revoked_scope = revoked_owner.scope();
    let revoked_config = config(revoked_scope.clone());
    let mut revoked_service = HarnessBackedContextService::new(
        HarnessOwnerClient::new(
            revoked_owner,
            ProtectedAuthReference::new("owner-key-ref").expect("auth ref"),
        ),
        revoked_grants,
        revoked_config,
    )
    .expect("revoked service");
    let revoked = revoked_service
        .create_draft(
            &request("editor", FULL_TOKEN, 100),
            &revoked_scope.run_id.replace("fixture-run", "revision-1"),
        )
        .expect_err("revoked grant");
    assert_eq!(revoked.code, "grant_revoked");
}

#[test]
fn expiry_foreign_references_and_restart_receipts_are_bounded() {
    let owner = RecordingOwner::new(ControlPlane::synthetic());
    let scope = owner.scope();
    let auth = ProtectedAuthReference::new("owner-key-ref").expect("auth ref");
    let mut service = HarnessBackedContextService::new(
        HarnessOwnerClient::new(owner, auth),
        grants(&scope, FULL_TOKEN, 10_000),
        config(scope.clone()),
    )
    .expect("service");
    let full = request("editor", FULL_TOKEN, 100);
    let active = service.state(&full).expect("state").active_revision_id;
    let draft = service.create_draft(&full, &active).expect("draft");
    let foreign_item = ControlItemRef {
        item_id: "foreign-item".to_owned(),
        version: 1,
        sha256: "a".repeat(64),
    };
    let before_calls = service.owner().calls.len();
    let foreign = service
        .edit_draft(
            &full,
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope: scope.clone(),
                draft_id: draft.draft_id,
                expected_draft_version: 1,
                expected_active_revision_id: active,
                operations: vec![ControlOperation::IncludeItem { item: foreign_item }],
            },
        )
        .expect_err("foreign item must fail before apply_patch");
    assert_eq!(foreign.code, "foreign_reference");
    assert_eq!(
        service.owner().calls[before_calls..]
            .iter()
            .filter(|operation| operation.as_str() == "apply_patch")
            .count(),
        0
    );

    let short_scope = service.owner().scope();
    let mut short_grants = GrantRegistry::new();
    short_grants
        .issue(
            "short-edit",
            FacadePermission::Edit,
            short_scope.clone(),
            SHORT_TOKEN,
            10,
        )
        .expect("short grant");
    let mut short_service = HarnessBackedContextService::new(
        HarnessOwnerClient::new(
            ControlPlane::synthetic(),
            ProtectedAuthReference::new("owner-key-ref").expect("auth ref"),
        ),
        short_grants,
        config(short_scope.clone()),
    )
    .expect("short service");
    let short_active = short_service.owner().state().active_revision_id;
    let expired = short_service
        .create_draft(&request("short-editor", SHORT_TOKEN, 10), &short_active)
        .expect_err("expired grant");
    assert_eq!(expired.code, "grant_expired");

    let capabilities = service
        .capabilities(&request("reader", FULL_TOKEN, 100))
        .expect("capabilities");
    assert_eq!(capabilities.composition, "harness_backed");
    assert!(!capabilities.legacy_demo);
    assert!(!capabilities.duplicate_scheduler);
}

#[test]
fn invalid_configuration_cannot_smuggle_urls_or_unapproved_private_retention() {
    assert!(ProtectedAuthReference::new("https://upstream.example").is_err());
    let request_debug = format!("{:?}", request("studio", FULL_TOKEN, 100));
    assert!(!request_debug.contains("full-capability"));
    assert!(!request_debug.contains("request-csrf"));
    let scope = ControlPlane::synthetic().scope().clone();
    let digest = SecretDigest::from_secret(CSRF).expect("digest");
    let private = HarnessFacadeConfig::new(
        scope.clone(),
        HOST,
        Some(ORIGIN.to_owned()),
        Some(digest),
        context_service::RetentionPolicy {
            mode: context_service::RetentionMode::PrivateEncrypted,
            policy_accepted: false,
            authenticated_encryption: false,
        },
    );
    assert!(private.is_err());
    let config = HarnessFacadeConfig::new(
        scope,
        HOST,
        Some(ORIGIN.to_owned()),
        Some(digest),
        Default::default(),
    )
    .expect("safe default");
    assert_eq!(config.retention.mode, context_service::RetentionMode::Off);
}

#[test]
fn http_adapter_enforces_same_origin_and_never_accepts_secret_query_fields() {
    let owner = RecordingOwner::new(ControlPlane::synthetic());
    let scope = owner.scope();
    let mut service = HarnessBackedContextService::new(
        HarnessOwnerClient::new(
            owner,
            ProtectedAuthReference::new("owner-key-ref").expect("auth ref"),
        ),
        grants(&scope, FULL_TOKEN, 10_000),
        config(scope.clone()),
    )
    .expect("service");
    let headers = vec![
        ("host".to_owned(), HOST.to_owned()),
        ("origin".to_owned(), ORIGIN.to_owned()),
        (
            "authorization".to_owned(),
            format!("Bearer {}", String::from_utf8_lossy(FULL_TOKEN)),
        ),
        ("x-principal".to_owned(), "studio".to_owned()),
        (
            "x-csrf-token".to_owned(),
            String::from_utf8_lossy(CSRF).into_owned(),
        ),
    ];
    let capabilities = service.handle_http_at(
        &context_service::HttpRequest {
            method: "GET".to_owned(),
            target: format!("/v2/runs/{}/context-control/capabilities", scope.run_id),
            headers: headers.clone(),
            body: Vec::new(),
        },
        100,
    );
    assert_eq!(capabilities.status, 200);

    let mut forged = headers.clone();
    forged[1].1 = "https://attacker.test".to_owned();
    let denied = service.handle_http_at(
        &context_service::HttpRequest {
            method: "POST".to_owned(),
            target: format!("/v2/runs/{}/context-control/drafts", scope.run_id),
            headers: forged,
            body: br#"{"expected_active_revision_id":"revision-1"}"#.to_vec(),
        },
        100,
    );
    assert_eq!(denied.status, 403);

    let missing_csrf = headers
        .iter()
        .filter(|(key, _)| key != "x-csrf-token")
        .cloned()
        .collect();
    let csrf_denied = service.handle_http_at(
        &context_service::HttpRequest {
            method: "POST".to_owned(),
            target: format!("/v2/runs/{}/context-control/drafts", scope.run_id),
            headers: missing_csrf,
            body: br#"{"expected_active_revision_id":"revision-1"}"#.to_vec(),
        },
        100,
    );
    assert_eq!(csrf_denied.status, 403);

    let secret_query = service.handle_http_at(
        &context_service::HttpRequest {
            method: "GET".to_owned(),
            target: format!(
                "/v2/runs/{}/context-control/capabilities?token=leaked",
                scope.run_id
            ),
            headers: headers.clone(),
            body: Vec::new(),
        },
        100,
    );
    assert_eq!(secret_query.status, 400);

    let body_too_large = service.handle_http_at(
        &context_service::HttpRequest {
            method: "POST".to_owned(),
            target: format!("/v2/runs/{}/context-control/drafts", scope.run_id),
            headers,
            body: vec![b'x'; context_service::MAX_HTTP_BODY_BYTES + 1],
        },
        100,
    );
    assert_eq!(body_too_large.status, 413);
}
