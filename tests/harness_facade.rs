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
    commit_effect_count: usize,
    drop_next_commit_reply: bool,
    flood_receipts: bool,
    next_flood_receipt: u64,
    hostile_locked_reason: Option<String>,
    hostile_state_model: Option<String>,
    hostile_preview_base: Option<String>,
    oversized_items: bool,
}

impl RecordingOwner {
    fn new(inner: ControlPlane) -> Self {
        Self {
            inner,
            calls: Vec::new(),
            commit_effects: BTreeSet::new(),
            commit_effect_count: 0,
            drop_next_commit_reply: false,
            flood_receipts: false,
            next_flood_receipt: 0,
            hostile_locked_reason: None,
            hostile_state_model: None,
            hostile_preview_base: None,
            oversized_items: false,
        }
    }

    fn with_lost_commit_reply(mut self) -> Self {
        self.drop_next_commit_reply = true;
        self
    }

    fn with_receipt_flood(mut self) -> Self {
        self.flood_receipts = true;
        self
    }

    fn with_hostile_projections(mut self) -> Self {
        self.hostile_locked_reason = Some("/srv/private/owner-secret".to_owned());
        self.hostile_state_model = Some("/srv/private/model".to_owned());
        self.hostile_preview_base = Some("/srv/private/base".to_owned());
        self
    }

    fn with_oversized_items(mut self) -> Self {
        self.oversized_items = true;
        self
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
        let mut state = HarnessOwnerPort::state(&mut self.inner, auth, scope)?;
        if let Some(model) = self.hostile_state_model.clone() {
            state.boundary.as_mut().expect("fixture boundary").model = model;
        }
        Ok(state)
    }

    fn eligible_items(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
    ) -> Result<Vec<EligibleItem>, OwnerError> {
        self.record("eligible_items");
        let mut items = HarnessOwnerPort::eligible_items(&mut self.inner, auth, scope)?;
        if let Some(reason) = self.hostile_locked_reason.clone()
            && let Some(item) = items.first_mut()
        {
            item.locked_reason = Some(reason);
        }
        if self.oversized_items {
            let first = items.first().cloned().expect("fixture item");
            items.resize(64, first);
        }
        Ok(items)
    }

    fn revisions(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
    ) -> Result<Vec<Revision>, OwnerError> {
        self.record("revisions");
        HarnessOwnerPort::revisions(&mut self.inner, auth, scope)
    }

    fn get_revision(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
        revision_id: &str,
    ) -> Result<Revision, OwnerError> {
        self.record("get_revision");
        HarnessOwnerPort::get_revision(&mut self.inner, auth, scope, revision_id)
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

    fn get_receipt_by_idempotency_key(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &ControlScope,
        idempotency_key: &str,
    ) -> Result<Receipt, OwnerError> {
        self.record("get_receipt_by_idempotency_key");
        HarnessOwnerPort::get_receipt_by_idempotency_key(
            &mut self.inner,
            auth,
            scope,
            idempotency_key,
        )
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
        let mut preview = HarnessOwnerPort::create_preview(
            &mut self.inner,
            auth,
            scope,
            draft_id,
            expected_draft_version,
            applicable_requested,
            expected_control_version,
            risk_ack,
        )?;
        if let Some(base_revision) = self.hostile_preview_base.clone() {
            preview.base_revision_id = base_revision;
        }
        Ok(preview)
    }

    fn pause(
        &mut self,
        auth: &ProtectedAuthReference,
        command: ControlCommand,
    ) -> Result<Receipt, OwnerError> {
        self.record("pause");
        if self.flood_receipts {
            self.next_flood_receipt = self.next_flood_receipt.saturating_add(1);
            return Ok(Receipt {
                schema: "ascension.context-control.receipt.v1".to_owned(),
                command_id: format!("flood-receipt-{}", self.next_flood_receipt),
                scope: command.scope,
                kind: "pause".to_owned(),
                status: "completed".to_owned(),
                effect: "pause_requested".to_owned(),
                control_version: command.expected_control_version,
                active_revision_id: "revision-1".to_owned(),
                paused: true,
                reason_code: None,
                observed_at: "2030-01-01T00:00:00Z".to_owned(),
            });
        }
        HarnessOwnerPort::pause(&mut self.inner, auth, command)
    }

    fn commit(
        &mut self,
        auth: &ProtectedAuthReference,
        command: ControlCommand,
    ) -> Result<Receipt, OwnerError> {
        self.record("commit");
        let events_before = self.inner.events().len();
        let result = HarnessOwnerPort::commit(&mut self.inner, auth, command);
        if self.inner.events().len() > events_before {
            self.commit_effect_count += 1;
        }
        let receipt = result?;
        self.commit_effects.insert(receipt.command_id.clone());
        if self.drop_next_commit_reply {
            self.drop_next_commit_reply = false;
            return Err(OwnerError::Unavailable);
        }
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

fn http_request(
    scope: &ControlScope,
    method: &str,
    tail: &str,
    token: &[u8],
    body: Vec<u8>,
) -> context_service::HttpRequest {
    context_service::HttpRequest {
        method: method.to_owned(),
        target: format!("/v2/runs/{}/context-control/{tail}", scope.run_id),
        headers: vec![
            ("host".to_owned(), HOST.to_owned()),
            ("origin".to_owned(), ORIGIN.to_owned()),
            (
                "authorization".to_owned(),
                format!("Bearer {}", String::from_utf8_lossy(token)),
            ),
            ("x-principal".to_owned(), "studio".to_owned()),
            (
                "x-csrf-token".to_owned(),
                String::from_utf8_lossy(CSRF).into_owned(),
            ),
        ],
        body,
    }
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

fn command_from_state(
    state: &ControlState,
    scope: &ControlScope,
    kind: &str,
    key: &str,
) -> ControlCommand {
    ControlCommand {
        schema: "ascension.context-control.command.v1".to_owned(),
        scope: scope.clone(),
        idempotency_key: key.to_owned(),
        command_window_id: state.command_window_id.clone(),
        expected_control_version: state.control_version,
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

fn objective_revision() -> (RecordingOwner, ControlScope, String) {
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
    let full = request("objective-editor", FULL_TOKEN, 100);
    let active = service.state(&full).expect("state").active_revision_id;
    let draft = service.create_draft(&full, &active).expect("draft");
    let objective = service
        .edit_draft(
            &full,
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope: scope.clone(),
                draft_id: draft.draft_id.clone(),
                expected_draft_version: draft.version,
                expected_active_revision_id: active.clone(),
                operations: vec![ControlOperation::SetObjective {
                    text: "objective for restore authorization".to_owned(),
                }],
            },
        )
        .expect("objective");
    let pause = command(
        &service.owner().inner,
        "pause",
        "objective-pause",
        service.owner().inner.state().control_version,
    );
    service.pause(&full, pause).expect("pause");
    let preview = service
        .preview(
            &full,
            PreviewRequest {
                scope: scope.clone(),
                draft_id: objective.draft_id.clone(),
                expected_draft_version: objective.version,
                applicable_requested: true,
                expected_control_version: service.owner().inner.state().control_version,
                unknown_total_risk_acknowledged: true,
            },
        )
        .expect("preview");
    let mut commit = command(
        &service.owner().inner,
        "commit",
        "objective-commit",
        service.owner().inner.state().control_version,
    );
    commit.expected_active_revision_id = Some(active);
    commit.preview_id = Some(preview.preview_id.clone());
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    let committed = service.held_commit(&full, commit).expect("commit");
    let mut resume = command(
        &service.owner().inner,
        "resume",
        "objective-resume",
        service.owner().inner.state().control_version,
    );
    resume.expected_active_revision_id = Some(committed.active_revision_id.clone());
    resume.expected_preview_id = Some(preview.preview_id);
    service.resume(&full, resume).expect("resume");
    let source_revision = committed.active_revision_id;
    let owner = service.into_owner();
    (owner, scope, source_revision)
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
    assert_eq!(service.owner().commit_effect_count, 1);

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
    assert_eq!(recovered.owner().commit_effect_count, 0);
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
fn restore_objective_requires_a_grant_before_owner_forwarding() {
    let (owner, scope, source_revision) = objective_revision();
    let mut service = HarnessBackedContextService::new(
        HarnessOwnerClient::new(
            owner,
            ProtectedAuthReference::new("owner-key-ref").expect("auth ref"),
        ),
        edit_grant(&scope, EDIT_TOKEN, 10_000),
        config(scope.clone()),
    )
    .expect("ordinary edit service");
    let ordinary = request("ordinary-editor", EDIT_TOKEN, 100);
    let draft = service
        .create_draft(&ordinary, &source_revision)
        .expect("draft");
    let before = service.owner().calls.len();
    let error = service
        .edit_draft(
            &ordinary,
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope,
                draft_id: draft.draft_id,
                expected_draft_version: draft.version,
                expected_active_revision_id: source_revision.clone(),
                operations: vec![ControlOperation::RestoreConfiguration {
                    source_revision_id: source_revision,
                }],
            },
        )
        .expect_err("restore objective requires the separate grant");
    assert_eq!(error.code, "objective_grant_required");
    assert_eq!(
        service.owner().calls[before..]
            .iter()
            .filter(|operation| operation.as_str() == "apply_patch")
            .count(),
        0
    );
}

#[test]
fn cached_draft_version_rejects_stale_patch_before_owner_mutation() {
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
    let full = request("editor", FULL_TOKEN, 100);
    let active = service.state(&full).expect("state").active_revision_id;
    let draft = service.create_draft(&full, &active).expect("draft");
    let item = history(&service.owner().inner);
    let patch = ControlPatch {
        schema: "ascension.context-control.patch.v1".to_owned(),
        scope: scope.clone(),
        draft_id: draft.draft_id.clone(),
        expected_draft_version: draft.version,
        expected_active_revision_id: active.clone(),
        operations: vec![ControlOperation::IncludeItem { item }],
    };
    service
        .edit_draft(&full, patch.clone())
        .expect("first edit");
    let before = service.owner().calls.len();
    let error = service
        .edit_draft(&full, patch)
        .expect_err("cached version is stale");
    assert_eq!(error.code, "stale_draft");
    assert_eq!(
        service.owner().calls[before..]
            .iter()
            .filter(|operation| operation.as_str() == "apply_patch")
            .count(),
        0
    );
}

#[test]
fn edit_only_note_references_stay_indexed_across_pin_update_and_unpin() {
    let owner = RecordingOwner::new(ControlPlane::synthetic());
    let scope = owner.scope();
    let mut service = HarnessBackedContextService::new(
        HarnessOwnerClient::new(
            owner,
            ProtectedAuthReference::new("owner-key-ref").expect("auth ref"),
        ),
        edit_grant(&scope, EDIT_TOKEN, 10_000),
        config(scope.clone()),
    )
    .expect("edit-only service");
    let edit_only = request("editor", EDIT_TOKEN, 100);
    let draft = service
        .create_draft(&edit_only, "revision-1")
        .expect("draft");
    let first = service
        .edit_draft(
            &edit_only,
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope: scope.clone(),
                draft_id: draft.draft_id.clone(),
                expected_draft_version: draft.version,
                expected_active_revision_id: "revision-1".to_owned(),
                operations: vec![ControlOperation::PutNote {
                    note_id: "edit-only-note".to_owned(),
                    expected_note_version: None,
                    text: "first note".to_owned(),
                    expires_at: "2030-01-01T00:00:00Z".to_owned(),
                }],
            },
        )
        .expect("create note");
    let first_ref = first.note_items.first().cloned().expect("created note ref");

    let pinned = service
        .edit_draft(
            &edit_only,
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope: scope.clone(),
                draft_id: first.draft_id.clone(),
                expected_draft_version: first.version,
                expected_active_revision_id: "revision-1".to_owned(),
                operations: vec![ControlOperation::PinItem {
                    item: first_ref.clone(),
                }],
            },
        )
        .expect("pin returned note reference");
    assert_eq!(pinned.pinned_item_ids, vec!["edit-only-note".to_owned()]);

    let updated = service
        .edit_draft(
            &edit_only,
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope: scope.clone(),
                draft_id: pinned.draft_id.clone(),
                expected_draft_version: pinned.version,
                expected_active_revision_id: "revision-1".to_owned(),
                operations: vec![ControlOperation::PutNote {
                    note_id: "edit-only-note".to_owned(),
                    expected_note_version: Some(first_ref.version),
                    text: "updated note".to_owned(),
                    expires_at: "2030-01-01T00:00:00Z".to_owned(),
                }],
            },
        )
        .expect("update note");
    let updated_ref = updated
        .note_items
        .first()
        .cloned()
        .expect("updated note ref");
    assert_ne!(updated_ref.version, first_ref.version);

    let unpinned = service
        .edit_draft(
            &edit_only,
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope,
                draft_id: updated.draft_id,
                expected_draft_version: updated.version,
                expected_active_revision_id: "revision-1".to_owned(),
                operations: vec![ControlOperation::UnpinItem { item: updated_ref }],
            },
        )
        .expect("unpin returned updated note reference");
    assert!(unpinned.pinned_item_ids.is_empty());
}

#[test]
fn write_driven_reference_caches_evict_and_revalidate_at_fixed_capacity() {
    let owner = RecordingOwner::new(ControlPlane::synthetic()).with_receipt_flood();
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
    let full = request("editor", FULL_TOKEN, 100);
    let mut first_draft_id = None;
    for index in 0..(context_service::MAX_FACADE_CACHE_ENTRIES + 4) {
        let draft = service
            .create_draft(&full, "revision-1")
            .expect("draft beyond cache capacity");
        if index == 0 {
            first_draft_id = Some(draft.draft_id);
        }
    }
    let status = service.cache_status();
    assert_eq!(
        status.drafts,
        context_service::MAX_FACADE_CACHE_ENTRIES,
        "draft cache must evict oldest entries"
    );
    assert!(status.drafts <= context_service::MAX_FACADE_CACHE_ENTRIES);

    let first_draft_id = first_draft_id.expect("first draft id");
    let before_revalidation = service.owner().calls.len();
    service
        .read_draft(&full, &first_draft_id)
        .expect("evicted draft is revalidated through owner");
    assert!(
        service.owner().calls[before_revalidation..]
            .iter()
            .any(|operation| operation == "get_draft")
    );

    let draft = service
        .read_draft(&full, &first_draft_id)
        .expect("draft for preview flood");
    for _ in 0..(context_service::MAX_FACADE_CACHE_ENTRIES + 4) {
        service
            .preview(
                &full,
                PreviewRequest {
                    scope: scope.clone(),
                    draft_id: draft.draft_id.clone(),
                    expected_draft_version: draft.version,
                    applicable_requested: false,
                    expected_control_version: 0,
                    unknown_total_risk_acknowledged: false,
                },
            )
            .expect("preview beyond cache capacity");
    }
    let status = service.cache_status();
    assert_eq!(
        status.previews,
        context_service::MAX_FACADE_CACHE_ENTRIES,
        "preview cache must evict oldest entries"
    );

    let state = service.owner().inner.state();
    for index in 0..(context_service::MAX_FACADE_CACHE_ENTRIES + 4) {
        let receipt = service
            .pause(
                &full,
                command_from_state(&state, &scope, "pause", &format!("flood-receipt-{index}")),
            )
            .expect("receipt beyond cache capacity");
        assert_eq!(receipt.kind, "pause");
    }
    let status = service.cache_status();
    assert_eq!(
        status.receipts,
        context_service::MAX_FACADE_CACHE_ENTRIES,
        "receipt cache must evict oldest entries"
    );
}

#[test]
fn evicted_revision_is_revalidated_without_fetching_the_full_history() {
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
    let full = request("revision-editor", FULL_TOKEN, 100);
    let history_item = history(&service.owner().inner);

    for index in 0..(context_service::MAX_FACADE_CACHE_ENTRIES + 1) {
        let active = service.owner().inner.state().active_revision_id;
        let draft = service
            .create_draft(&full, &active)
            .expect("draft for revision history");
        let operation = if index % 2 == 0 {
            ControlOperation::IncludeItem {
                item: history_item.clone(),
            }
        } else {
            ControlOperation::ExcludeItem {
                item: history_item.clone(),
            }
        };
        let edited = service
            .edit_draft(
                &full,
                ControlPatch {
                    schema: "ascension.context-control.patch.v1".to_owned(),
                    scope: scope.clone(),
                    draft_id: draft.draft_id,
                    expected_draft_version: draft.version,
                    expected_active_revision_id: active.clone(),
                    operations: vec![operation],
                },
            )
            .expect("revision edit");
        let pause = command(
            &service.owner().inner,
            "pause",
            &format!("evicted-revision-pause-{index}"),
            service.owner().inner.state().control_version,
        );
        service.pause(&full, pause).expect("pause");
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
            .expect("applicable preview");
        let mut commit = command(
            &service.owner().inner,
            "commit",
            &format!("evicted-revision-commit-{index}"),
            service.owner().inner.state().control_version,
        );
        commit.expected_active_revision_id = Some(active);
        commit.preview_id = Some(preview.preview_id.clone());
        commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
        let committed = service.held_commit(&full, commit).expect("commit");
        let mut resume = command(
            &service.owner().inner,
            "resume",
            &format!("evicted-revision-resume-{index}"),
            service.owner().inner.state().control_version,
        );
        resume.expected_active_revision_id = Some(committed.active_revision_id.clone());
        resume.expected_preview_id = Some(preview.preview_id);
        service.resume(&full, resume).expect("resume");
    }

    assert!(
        service.owner().inner.revisions().len() > context_service::MAX_FACADE_CACHE_ENTRIES,
        "owner retains more revisions than the facade cache"
    );
    let active = service.owner().inner.state().active_revision_id;
    let draft = service
        .create_draft(&full, &active)
        .expect("draft for historical restore");
    let before_restore = service.owner().calls.len();
    let restored = service
        .edit_draft(
            &full,
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope,
                draft_id: draft.draft_id,
                expected_draft_version: draft.version,
                expected_active_revision_id: active,
                operations: vec![ControlOperation::RestoreConfiguration {
                    source_revision_id: "revision-1".to_owned(),
                }],
            },
        )
        .expect("evicted revision is restored");
    assert!(restored.selected_items.is_empty());
    assert!(
        service.owner().calls[before_restore..]
            .iter()
            .any(|operation| operation == "get_revision"),
        "historical restore must use the scoped lookup"
    );
    assert!(
        !service.owner().calls[before_restore..]
            .iter()
            .any(|operation| operation == "revisions"),
        "historical restore must not refresh the full revision collection"
    );
}

#[test]
fn hostile_owner_projections_are_redacted_or_rejected_and_responses_are_capped() {
    let owner = RecordingOwner::new(ControlPlane::synthetic()).with_hostile_projections();
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
    let full = request("reader", FULL_TOKEN, 100);
    let items = service.eligible_items(&full, false).expect("items");
    assert!(items[0].locked_reason.is_none());
    assert!(
        !serde_json::to_string(&items)
            .expect("items JSON")
            .contains("owner-secret")
    );
    let state = service.state(&full).expect_err("host path is not metadata");
    assert_eq!(state.code, "owner_invalid_state");
    assert!(!state.to_string().contains("/srv/private"));

    let draft = service
        .create_draft(&full, "revision-1")
        .expect("draft for preview");
    let preview = service
        .preview(
            &full,
            PreviewRequest {
                scope: scope.clone(),
                draft_id: draft.draft_id,
                expected_draft_version: draft.version,
                applicable_requested: false,
                expected_control_version: 0,
                unknown_total_risk_acknowledged: false,
            },
        )
        .expect_err("host path is not a preview reference");
    assert_eq!(preview.code, "owner_invalid_preview");
    assert!(!preview.to_string().contains("/srv/private"));

    let oversized_owner = RecordingOwner::new(ControlPlane::synthetic()).with_oversized_items();
    let oversized_scope = oversized_owner.scope();
    let mut oversized = HarnessBackedContextService::new(
        HarnessOwnerClient::new(
            oversized_owner,
            ProtectedAuthReference::new("owner-key-ref").expect("auth ref"),
        ),
        grants(&oversized_scope, FULL_TOKEN, 10_000),
        config(oversized_scope.clone()),
    )
    .expect("oversized service");
    let response = oversized.handle_http_at(
        &context_service::HttpRequest {
            method: "GET".to_owned(),
            target: format!(
                "/v2/runs/{}/context-control/eligible-items",
                oversized_scope.run_id
            ),
            headers: vec![
                ("host".to_owned(), HOST.to_owned()),
                ("origin".to_owned(), ORIGIN.to_owned()),
                (
                    "authorization".to_owned(),
                    format!("Bearer {}", String::from_utf8_lossy(FULL_TOKEN)),
                ),
                ("x-principal".to_owned(), "reader".to_owned()),
            ],
            body: Vec::new(),
        },
        100,
    );
    assert_eq!(response.status, 503);
    assert!(response.body.len() <= context_service::MAX_FACADE_RESPONSE_BYTES);
    let body: serde_json::Value = serde_json::from_slice(&response.body).expect("error JSON");
    assert_eq!(body["error"]["code"], "response_too_large");
}

#[test]
fn lost_commit_reply_recovers_receipt_after_process_restart_without_repeating_effect() {
    let owner = RecordingOwner::new(ControlPlane::synthetic()).with_lost_commit_reply();
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
    let full = request("editor", FULL_TOKEN, 100);
    let active = service.state(&full).expect("state").active_revision_id;
    let draft = service.create_draft(&full, &active).expect("draft");
    let item = history(&service.owner().inner);
    let edited = service
        .edit_draft(
            &full,
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope: scope.clone(),
                draft_id: draft.draft_id,
                expected_draft_version: draft.version,
                expected_active_revision_id: active.clone(),
                operations: vec![ControlOperation::IncludeItem { item }],
            },
        )
        .expect("edit");
    let pause = command(
        &service.owner().inner,
        "pause",
        "lost-reply-pause",
        service.owner().inner.state().control_version,
    );
    service.pause(&full, pause).expect("pause");
    let preview = service
        .preview(
            &full,
            PreviewRequest {
                scope: scope.clone(),
                draft_id: edited.draft_id,
                expected_draft_version: edited.version,
                applicable_requested: true,
                expected_control_version: service.owner().inner.state().control_version,
                unknown_total_risk_acknowledged: true,
            },
        )
        .expect("preview");
    let mut commit = command(
        &service.owner().inner,
        "commit",
        "lost-reply-commit",
        service.owner().inner.state().control_version,
    );
    commit.expected_active_revision_id = Some(active);
    commit.preview_id = Some(preview.preview_id);
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    let lost = service
        .held_commit(&full, commit.clone())
        .expect_err("reply is intentionally lost after apply");
    assert_eq!(lost.code, "owner_unavailable");
    assert_eq!(service.owner().commit_effect_count, 1);

    let journal = service
        .owner()
        .inner
        .export_journal()
        .expect("durable journal");
    let recovered_owner =
        RecordingOwner::new(ControlPlane::recover_journal(&journal).expect("restart recovery"));
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
    let commit_receipt_id = service
        .owner()
        .inner
        .receipts()
        .into_iter()
        .find(|receipt| receipt.kind == "commit")
        .expect("commit receipt")
        .command_id;
    let receipt = recovered
        .read_receipt(&full, &commit_receipt_id)
        .expect("receipt recovered after restart");
    let replayed = recovered
        .held_commit(&full, commit)
        .expect("idempotent replay");
    assert_eq!(replayed, receipt);
    assert_eq!(recovered.owner().commit_effect_count, 0);
}

#[test]
fn http_receipt_recovery_uses_idempotency_key_after_lost_reply_and_write_revocation() {
    let owner = RecordingOwner::new(ControlPlane::synthetic()).with_lost_commit_reply();
    let scope = owner.scope();
    let mut grants = grants(&scope, FULL_TOKEN, 10_000);
    grants
        .issue(
            "metadata-only",
            FacadePermission::MetadataRead,
            scope.clone(),
            METADATA_TOKEN,
            10_000,
        )
        .expect("metadata grant");
    let mut service = HarnessBackedContextService::new(
        HarnessOwnerClient::new(
            owner,
            ProtectedAuthReference::new("owner-key-ref").expect("auth ref"),
        ),
        grants,
        config(scope.clone()),
    )
    .expect("service");

    let read_state = |service: &mut HarnessBackedContextService<RecordingOwner>| {
        let response = service.handle_http_at(
            &http_request(&scope, "GET", "state", FULL_TOKEN, Vec::new()),
            100,
        );
        assert_eq!(response.status, 200);
        serde_json::from_slice::<ControlState>(&response.body).expect("state response")
    };
    let state = read_state(&mut service);
    let draft_response = service.handle_http_at(
        &http_request(
            &scope,
            "POST",
            "drafts",
            FULL_TOKEN,
            br#"{"expected_active_revision_id":"revision-1"}"#.to_vec(),
        ),
        100,
    );
    assert_eq!(draft_response.status, 200);
    let draft: Draft = serde_json::from_slice(&draft_response.body).expect("draft response");

    let items_response = service.handle_http_at(
        &http_request(&scope, "GET", "eligible-items", FULL_TOKEN, Vec::new()),
        100,
    );
    assert_eq!(items_response.status, 200);
    let items: Vec<EligibleItem> =
        serde_json::from_slice(&items_response.body).expect("eligible-item response");
    let item = items
        .into_iter()
        .find(|item| item.item.item_id == "history-1")
        .expect("history item")
        .item;
    let patch = ControlPatch {
        schema: "ascension.context-control.patch.v1".to_owned(),
        scope: scope.clone(),
        draft_id: draft.draft_id.clone(),
        expected_draft_version: draft.version,
        expected_active_revision_id: state.active_revision_id.clone(),
        operations: vec![ControlOperation::IncludeItem { item }],
    };
    let edited_response = service.handle_http_at(
        &http_request(
            &scope,
            "POST",
            &format!("drafts/{}/operations", draft.draft_id),
            FULL_TOKEN,
            serde_json::to_vec(&patch).expect("patch JSON"),
        ),
        100,
    );
    assert_eq!(edited_response.status, 200);
    let edited: Draft = serde_json::from_slice(&edited_response.body).expect("edited response");

    let pause = command_from_state(&state, &scope, "pause", "http-lost-reply-pause");
    let pause_response = service.handle_http_at(
        &http_request(
            &scope,
            "POST",
            "pause",
            FULL_TOKEN,
            serde_json::to_vec(&pause).expect("pause JSON"),
        ),
        100,
    );
    assert_eq!(pause_response.status, 200);
    let paused: Receipt = serde_json::from_slice(&pause_response.body).expect("pause receipt");
    let paused_state = read_state(&mut service);
    assert_eq!(paused.kind, "pause");

    let preview_request = PreviewRequest {
        scope: scope.clone(),
        draft_id: edited.draft_id.clone(),
        expected_draft_version: edited.version,
        applicable_requested: true,
        expected_control_version: paused_state.control_version,
        unknown_total_risk_acknowledged: true,
    };
    let preview_response = service.handle_http_at(
        &http_request(
            &scope,
            "POST",
            "previews",
            FULL_TOKEN,
            serde_json::to_vec(&preview_request).expect("preview JSON"),
        ),
        100,
    );
    assert_eq!(preview_response.status, 200);
    let preview: Preview =
        serde_json::from_slice(&preview_response.body).expect("preview response");

    let mut commit = command_from_state(&paused_state, &scope, "commit", "http-lost-reply-commit");
    commit.expected_active_revision_id = Some(paused_state.active_revision_id.clone());
    commit.preview_id = Some(preview.preview_id);
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    let lost_response = service.handle_http_at(
        &http_request(
            &scope,
            "POST",
            "commits",
            FULL_TOKEN,
            serde_json::to_vec(&commit).expect("commit JSON"),
        ),
        100,
    );
    assert_eq!(lost_response.status, 503);
    let lost_body: serde_json::Value =
        serde_json::from_slice(&lost_response.body).expect("lost-reply error JSON");
    assert_eq!(lost_body["error"]["retryable"], false);
    assert_eq!(service.owner().commit_effect_count, 1);

    for grant_id in ["edit", "pause", "commit", "resume"] {
        service
            .grants_mut()
            .revoke(grant_id)
            .expect("revoke write grant");
    }
    let recovered_response = service.handle_http_at(
        &http_request(
            &scope,
            "GET",
            "commands/by-idempotency-key/http-lost-reply-commit",
            METADATA_TOKEN,
            Vec::new(),
        ),
        100,
    );
    assert_eq!(recovered_response.status, 200);
    let recovered_receipt: Receipt =
        serde_json::from_slice(&recovered_response.body).expect("recovered receipt");
    assert_eq!(recovered_receipt.kind, "commit");
    assert_eq!(service.owner().commit_effect_count, 1);
    assert!(
        service
            .owner()
            .calls
            .contains(&"get_receipt_by_idempotency_key".to_owned())
    );
    let query_response = service.handle_http_at(
        &http_request(
            &scope,
            "GET",
            "commands?idempotency_key=http-lost-reply-commit",
            METADATA_TOKEN,
            Vec::new(),
        ),
        100,
    );
    assert_eq!(query_response.status, 200);
}

#[test]
fn published_facade_openapi_is_local_and_matches_transport_shapes() {
    let document: serde_json::Value = serde_json::from_str(include_str!(
        "../contracts/context-control/harness-facade.openapi.json"
    ))
    .expect("facade OpenAPI JSON");
    assert_eq!(document["openapi"], "3.1.0");
    assert_eq!(
        document["x-response-max-bytes"],
        context_service::MAX_FACADE_RESPONSE_BYTES
    );
    let paths = &document["paths"];
    let create_request = &paths["/v2/runs/{run_id}/context-control/drafts"]["post"]["requestBody"]
        ["content"]["application/json"]["schema"];
    assert_eq!(
        create_request["$ref"],
        "#/components/schemas/CreateDraftRequest"
    );
    assert!(
        !create_request
            .get("required")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|required| required.iter().any(|field| field == "scope"))
    );
    assert_eq!(
        paths["/v2/runs/{run_id}/context-control/capabilities"]["get"]["responses"]["200"]["content"]
            ["application/json"]["schema"]["$ref"],
        "#/components/schemas/FacadeCapabilities"
    );
    assert_eq!(
        paths["/v2/runs/{run_id}/context-control/eligible-items"]["get"]["responses"]["200"]["content"]
            ["application/json"]["schema"]["type"],
        "array"
    );
    assert_eq!(
        paths["/v2/runs/{run_id}/context-control/commands"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"]["type"],
        "array"
    );
    assert_eq!(
        paths["/v2/runs/{run_id}/context-control/commands/by-idempotency-key/{idempotency_key}"]["get"]
            ["operationId"],
        "facadeReceiptRecovery"
    );
    let mut references = Vec::new();
    collect_refs(&document, &mut references);
    assert!(
        references
            .iter()
            .all(|reference| reference.starts_with("#/components/")),
        "facade OpenAPI must not import legacy external operation schemas"
    );
}

#[test]
fn transport_requests_and_responses_validate_against_published_facade_schemas() {
    let document: serde_json::Value = serde_json::from_str(include_str!(
        "../contracts/context-control/harness-facade.openapi.json"
    ))
    .expect("facade OpenAPI JSON");
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

    let capabilities_response = service.handle_http_at(
        &facade_http_request(
            "GET",
            format!("/v2/runs/{}/context-control/capabilities", scope.run_id),
            Vec::new(),
        ),
        100,
    );
    assert_eq!(capabilities_response.status, 200);
    let capabilities: serde_json::Value =
        serde_json::from_slice(&capabilities_response.body).expect("capabilities response");
    validate_openapi_value(
        &document,
        &document["components"]["schemas"]["FacadeCapabilities"],
        &capabilities,
    );

    let eligible_response = service.handle_http_at(
        &facade_http_request(
            "GET",
            format!("/v2/runs/{}/context-control/eligible-items", scope.run_id),
            Vec::new(),
        ),
        100,
    );
    assert_eq!(eligible_response.status, 200);
    let eligible: serde_json::Value =
        serde_json::from_slice(&eligible_response.body).expect("eligible response");
    validate_openapi_value(
        &document,
        &serde_json::json!({
            "type": "array",
            "items": {"$ref": "#/components/schemas/EligibleItem"}
        }),
        &eligible,
    );

    let create_body = serde_json::json!({
        "expected_active_revision_id": "revision-1"
    });
    validate_openapi_value(
        &document,
        &document["components"]["schemas"]["CreateDraftRequest"],
        &create_body,
    );
    let create_response = service.handle_http_at(
        &facade_http_request(
            "POST",
            format!("/v2/runs/{}/context-control/drafts", scope.run_id),
            serde_json::to_vec(&create_body).expect("create body"),
        ),
        100,
    );
    assert_eq!(create_response.status, 200);
    let draft: serde_json::Value =
        serde_json::from_slice(&create_response.body).expect("draft response");
    validate_openapi_value(
        &document,
        &document["components"]["schemas"]["Draft"],
        &draft,
    );
}

fn facade_http_request(
    method: &str,
    target: String,
    body: Vec<u8>,
) -> context_service::HttpRequest {
    let mut headers = vec![
        ("host".to_owned(), HOST.to_owned()),
        ("origin".to_owned(), ORIGIN.to_owned()),
        (
            "authorization".to_owned(),
            format!("Bearer {}", String::from_utf8_lossy(FULL_TOKEN)),
        ),
        ("x-principal".to_owned(), "schema-checker".to_owned()),
    ];
    if method == "POST" {
        headers.push((
            "x-csrf-token".to_owned(),
            String::from_utf8_lossy(CSRF).into(),
        ));
    }
    context_service::HttpRequest {
        method: method.to_owned(),
        target,
        headers,
        body,
    }
}

fn validate_openapi_value(
    document: &serde_json::Value,
    schema: &serde_json::Value,
    value: &serde_json::Value,
) {
    if let Err(error) = validate_openapi_value_inner(document, schema, value) {
        panic!("value does not satisfy the published facade schema: {error}");
    }
}

fn validate_openapi_value_inner(
    document: &serde_json::Value,
    schema: &serde_json::Value,
    value: &serde_json::Value,
) -> Result<(), String> {
    let schema = if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
        resolve_openapi_ref(document, reference)?
    } else {
        schema
    };
    if let Some(variants) = schema.get("anyOf").and_then(serde_json::Value::as_array) {
        if variants
            .iter()
            .any(|variant| validate_openapi_value_inner(document, variant, value).is_ok())
        {
            return Ok(());
        }
        return Err("no anyOf branch matched".to_owned());
    }
    if let Some(variants) = schema.get("oneOf").and_then(serde_json::Value::as_array) {
        let matches = variants
            .iter()
            .filter(|variant| validate_openapi_value_inner(document, variant, value).is_ok())
            .count();
        if matches == 1 {
            return Ok(());
        }
        return Err(format!("oneOf matched {matches} branches"));
    }
    if let Some(schemas) = schema.get("allOf").and_then(serde_json::Value::as_array) {
        for branch in schemas {
            validate_openapi_value_inner(document, branch, value)?;
        }
    }
    if let Some(expected) = schema.get("const")
        && expected != value
    {
        return Err(format!("expected const {expected}, got {value}"));
    }
    if let Some(values) = schema.get("enum").and_then(serde_json::Value::as_array)
        && !values.iter().any(|expected| expected == value)
    {
        return Err(format!("value {value} is outside enum"));
    }
    if let Some(types) = schema.get("type") {
        let matches = match types {
            serde_json::Value::String(kind) => value_matches_openapi_type(value, kind),
            serde_json::Value::Array(kinds) => kinds.iter().any(|kind| {
                kind.as_str()
                    .is_some_and(|kind| value_matches_openapi_type(value, kind))
            }),
            _ => false,
        };
        if !matches {
            return Err(format!("value {value} has the wrong JSON type"));
        }
    }
    if let Some(required) = schema.get("required").and_then(serde_json::Value::as_array) {
        let object = value
            .as_object()
            .ok_or_else(|| "required properties need an object".to_owned())?;
        for field in required.iter().filter_map(serde_json::Value::as_str) {
            if !object.contains_key(field) {
                return Err(format!("required property {field} is missing"));
            }
        }
    }
    if let Some(properties) = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
    {
        let object = value
            .as_object()
            .ok_or_else(|| "properties need an object".to_owned())?;
        if schema
            .get("additionalProperties")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
            && object.keys().any(|key| !properties.contains_key(key))
        {
            return Err("object contains an additional property".to_owned());
        }
        for (key, property_schema) in properties {
            if let Some(property) = object.get(key) {
                validate_openapi_value_inner(document, property_schema, property)?;
            }
        }
    }
    if let Some(items) = schema.get("items") {
        let array = value
            .as_array()
            .ok_or_else(|| "items need an array".to_owned())?;
        for item in array {
            validate_openapi_value_inner(document, items, item)?;
        }
    }
    if let Some(length) = schema.get("minLength").and_then(serde_json::Value::as_u64)
        && value
            .as_str()
            .is_some_and(|text| text.chars().count() < length as usize)
    {
        return Err("string is shorter than minLength".to_owned());
    }
    if let Some(length) = schema.get("maxLength").and_then(serde_json::Value::as_u64)
        && value
            .as_str()
            .is_some_and(|text| text.chars().count() > length as usize)
    {
        return Err("string is longer than maxLength".to_owned());
    }
    if let Some(length) = schema.get("minItems").and_then(serde_json::Value::as_u64)
        && value
            .as_array()
            .is_some_and(|items| items.len() < length as usize)
    {
        return Err("array is shorter than minItems".to_owned());
    }
    if let Some(length) = schema.get("maxItems").and_then(serde_json::Value::as_u64)
        && value
            .as_array()
            .is_some_and(|items| items.len() > length as usize)
    {
        return Err("array is longer than maxItems".to_owned());
    }
    Ok(())
}

fn resolve_openapi_ref<'a>(
    document: &'a serde_json::Value,
    reference: &str,
) -> Result<&'a serde_json::Value, String> {
    let path = reference
        .strip_prefix("#/")
        .ok_or_else(|| format!("external reference {reference}"))?;
    let mut value = document;
    for part in path.split('/') {
        value = value
            .get(part)
            .ok_or_else(|| format!("unresolved reference {reference}"))?;
    }
    Ok(value)
}

fn value_matches_openapi_type(value: &serde_json::Value, kind: &str) -> bool {
    match kind {
        "array" => value.is_array(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_u64().is_some() || value.as_i64().is_some(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "object" => value.is_object(),
        "string" => value.is_string(),
        _ => false,
    }
}

fn collect_refs(value: &serde_json::Value, references: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(serde_json::Value::as_str) {
                references.push(reference.to_owned());
            }
            for child in object.values() {
                collect_refs(child, references);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                collect_refs(child, references);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
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
