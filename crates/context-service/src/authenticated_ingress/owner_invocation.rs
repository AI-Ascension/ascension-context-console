// SPDX-License-Identifier: MIT

//! Operation-selected admission for the typed Console18 owner protocol.

use super::{
    AuthenticatedIngress, AuthenticatedIngressError, PrincipalVerificationError,
    ProtectedOwnerCredentialResolver, VerifiedPrincipal,
};
use crate::control::Scope;
use crate::harness_context_owner_wire::ContextOwnerInvocationV2;
use crate::harness_facade::{FacadePermission, HarnessFacadeConfig};
use crate::http::HttpRequest;
use crate::owner_invocation_store::{
    AdmissionError, AdmissionUse, TrustedInvocationAdmission, required_console_permissions,
    required_harness_scopes,
};
use crate::protected_owner_credentials::{ResolvedOwnerCredential, harness_scope_for};
use crate::subject_grants::{AdmittedSubjectGrant, SubjectGrantStore};
use std::cell::Cell;
use std::collections::BTreeSet;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

mod current_identity;

/// A non-forgeable result pairing one Console admission with its exact owner resolution.
///
/// The route must pass both references onward; it must not resolve a second credential after
/// consuming the Console grants. No bearer or request is retained. The grant rows and verified
/// principal are kept privately so the route can re-query the grant store before claim/result
/// boundaries without substituting a newly selected authority.
pub(crate) struct AuthenticatedOwnerInvocation {
    admission: TrustedInvocationAdmission,
    principal: VerifiedPrincipal,
    grants: Vec<AdmittedSubjectGrant>,
    credential: ResolvedOwnerCredential,
    invocation: ContextOwnerInvocationV2,
    scope: Scope,
    use_kind: AdmissionUse,
    wall_clock_floor: Cell<u64>,
}

impl AuthenticatedOwnerInvocation {
    pub(crate) fn admission(&self) -> &TrustedInvocationAdmission {
        &self.admission
    }

    pub(crate) fn credential(&self) -> &ResolvedOwnerCredential {
        &self.credential
    }

    pub(crate) fn invocation(&self) -> &ContextOwnerInvocationV2 {
        &self.invocation
    }

    pub(crate) fn use_kind(&self) -> AdmissionUse {
        self.use_kind
    }
}

impl<V, G> AuthenticatedIngress<V, G>
where
    V: super::PrincipalVerifier,
    G: SubjectGrantStore,
{
    /// Admit one typed owner invocation using the operation's closed grant policy.
    ///
    /// The route supplies the use kind, but callers cannot provide or broaden permission lists.
    /// The monotonic lifetime begins before request validation, bearer verification, grant
    /// reservations, and protected-owner resolution. Wall time is sampled inside ingress around
    /// every potentially blocking trust boundary; request and invocation data never set it.
    pub(crate) fn admit_owner_invocation<R: ProtectedOwnerCredentialResolver>(
        &mut self,
        request: &HttpRequest,
        facade: &HarnessFacadeConfig,
        invocation: &ContextOwnerInvocationV2,
        use_kind: AdmissionUse,
        resolver: &mut R,
    ) -> Result<AuthenticatedOwnerInvocation, AuthenticatedIngressError> {
        let admission_started = Instant::now();
        let mut now = trusted_unix_seconds()?;
        invocation
            .validate()
            .map_err(|_| AuthenticatedIngressError::InvalidHttpRequest)?;
        facade
            .validate()
            .map_err(|_| AuthenticatedIngressError::InvalidConfiguration)?;

        let scope = invocation_scope(invocation);
        if facade.scope != scope {
            return Err(AuthenticatedIngressError::Unauthorized);
        }
        let (required, required_scopes) = selected_policy(invocation, use_kind)?;
        super::validation::validate_owner_http_envelope(request, facade, &required)?;
        let bearer = super::validation::bearer_from_headers(request)?;

        // Sample on both sides even when verification fails. This avoids passing a timestamp
        // captured before a slow verifier to the next authorization check.
        now = advance_wall_clock(&mut now)?;
        let verified = self.verifier.verify(bearer);
        now = advance_wall_clock(&mut now)?;
        let claims = verified.map_err(map_verifier_error)?;
        let principal = self.validate_claims(claims, now)?;
        validate_principal_for_invocation(&principal, invocation, now)?;

        now = advance_wall_clock(&mut now)?;
        ensure_principal_current(&principal, now)?;
        let first_reservation = self.grants.reserve(&principal, &scope, &required, &[], now);
        now = advance_wall_clock(&mut now)?;
        let admitted = first_reservation.map_err(super::validation::map_grant_error)?;
        validate_selected_grants(
            &admitted,
            &principal,
            &scope,
            &required,
            &required_scopes,
            now,
        )?;

        now = advance_wall_clock(&mut now)?;
        ensure_principal_current(&principal, now)?;
        validate_selected_grants(
            &admitted,
            &principal,
            &scope,
            &required,
            &required_scopes,
            now,
        )?;
        let resolved_result = resolver.resolve(&principal, &scope, &required_scopes, now);
        now = advance_wall_clock(&mut now)?;
        let resolved = resolved_result.map_err(super::validation::map_credential_error)?;
        ensure_principal_current(&principal, now)?;
        validate_selected_grants(
            &admitted,
            &principal,
            &scope,
            &required,
            &required_scopes,
            now,
        )?;
        super::validation::validate_owner_credential(
            &resolved, &principal, &scope, &required, now,
        )?;

        // Resolution may block while a grant is revoked or replaced. Re-reserve the same closed
        // policy and require byte-for-byte-equivalent authorization fields before sealing it.
        now = advance_wall_clock(&mut now)?;
        ensure_principal_current(&principal, now)?;
        let refreshed_result = self.grants.reserve(&principal, &scope, &required, &[], now);
        now = advance_wall_clock(&mut now)?;
        let refreshed = refreshed_result.map_err(super::validation::map_grant_error)?;
        ensure_principal_current(&principal, now)?;
        validate_selected_grants(
            &refreshed,
            &principal,
            &scope,
            &required,
            &required_scopes,
            now,
        )?;
        if !same_grants(&admitted, &refreshed) {
            return Err(AuthenticatedIngressError::GrantDenied);
        }
        super::validation::validate_owner_credential(
            &resolved, &principal, &scope, &required, now,
        )?;

        let admission = TrustedInvocationAdmission::from_verified_ingress(
            admission_started,
            &principal,
            &refreshed,
            &resolved,
            invocation,
            use_kind,
            now,
        )
        .map_err(map_admission_error)?;
        admission
            .validate_for(invocation, use_kind, now)
            .map_err(map_admission_error)?;
        Ok(AuthenticatedOwnerInvocation {
            admission,
            principal,
            grants: refreshed,
            credential: resolved,
            invocation: invocation.clone(),
            scope,
            use_kind,
            wall_clock_floor: Cell::new(now),
        })
    }

    /// Revalidate the sealed principal's exact grant rows against the live grant store.
    ///
    /// This refresh does not repeat bearer cryptographic verification, so it cannot prove
    /// revocation of the Console credential or signing key. The root route must reverify against
    /// current verifier state using its live request context and compare the same principal and
    /// credential tuple. This method also cannot create an atomic revocation fence with a separate
    /// owner-credential database. Callers must invoke it at the route's claim boundary and again
    /// before returning protected results; root integration must serialize those checks with its
    /// service-side revocation policy.
    pub(crate) fn revalidate_owner_invocation(
        &mut self,
        admitted: &AuthenticatedOwnerInvocation,
    ) -> Result<(), AuthenticatedIngressError> {
        let mut now = observe_wall_clock(&admitted.wall_clock_floor)?;
        let (required, required_scopes) = selected_policy(&admitted.invocation, admitted.use_kind)?;
        if invocation_scope(&admitted.invocation) != admitted.scope {
            return Err(AuthenticatedIngressError::Unauthorized);
        }
        validate_principal_for_invocation(&admitted.principal, &admitted.invocation, now)?;
        admitted
            .admission
            .validate_for(&admitted.invocation, admitted.use_kind, now)
            .map_err(map_admission_error)?;
        validate_selected_grants(
            &admitted.grants,
            &admitted.principal,
            &admitted.scope,
            &required,
            &required_scopes,
            now,
        )?;
        super::validation::validate_owner_credential(
            &admitted.credential,
            &admitted.principal,
            &admitted.scope,
            &required,
            now,
        )?;

        now = observe_wall_clock(&admitted.wall_clock_floor)?;
        ensure_principal_current(&admitted.principal, now)?;
        let refreshed_result =
            self.grants
                .reserve(&admitted.principal, &admitted.scope, &required, &[], now);
        now = observe_wall_clock(&admitted.wall_clock_floor)?;
        let refreshed = refreshed_result.map_err(super::validation::map_grant_error)?;
        validate_principal_for_invocation(&admitted.principal, &admitted.invocation, now)?;
        validate_selected_grants(
            &refreshed,
            &admitted.principal,
            &admitted.scope,
            &required,
            &required_scopes,
            now,
        )?;
        if !same_grants(&admitted.grants, &refreshed) {
            return Err(AuthenticatedIngressError::GrantDenied);
        }
        super::validation::validate_owner_credential(
            &admitted.credential,
            &admitted.principal,
            &admitted.scope,
            &required,
            now,
        )?;
        admitted
            .admission
            .validate_for(&admitted.invocation, admitted.use_kind, now)
            .map_err(map_admission_error)
    }
}

fn selected_policy(
    invocation: &ContextOwnerInvocationV2,
    use_kind: AdmissionUse,
) -> Result<(Vec<FacadePermission>, Vec<&'static str>), AuthenticatedIngressError> {
    let required = required_console_permissions(&invocation.operation, use_kind)
        .map_err(map_admission_error)?;
    let required_scopes =
        required_harness_scopes(&invocation.operation, use_kind).map_err(map_admission_error)?;
    let permission_scopes = required
        .iter()
        .map(|permission| harness_scope_for(*permission))
        .collect::<BTreeSet<_>>();
    let selector_scopes = required_scopes.iter().copied().collect::<BTreeSet<_>>();
    if required.is_empty()
        || required_scopes.is_empty()
        || selector_scopes.len() != required_scopes.len()
        || permission_scopes != selector_scopes
    {
        return Err(AuthenticatedIngressError::InvalidHttpRequest);
    }
    Ok((required, required_scopes))
}

fn validate_selected_grants(
    grants: &[AdmittedSubjectGrant],
    principal: &VerifiedPrincipal,
    scope: &Scope,
    required: &[FacadePermission],
    required_scopes: &[&'static str],
    now: u64,
) -> Result<(), AuthenticatedIngressError> {
    super::validation::validate_admitted_grants(grants, principal, scope, required, &[], now)?;
    let admitted_scopes = super::validation::unique_harness_scopes(grants)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let selected_scopes = required_scopes.iter().copied().collect::<BTreeSet<_>>();
    if admitted_scopes == selected_scopes {
        Ok(())
    } else {
        Err(AuthenticatedIngressError::GrantDenied)
    }
}

fn same_grants(left: &[AdmittedSubjectGrant], right: &[AdmittedSubjectGrant]) -> bool {
    fn sorted(grants: &[AdmittedSubjectGrant]) -> Vec<AdmittedSubjectGrant> {
        let mut sorted = grants.to_vec();
        sorted.sort_by(|left, right| {
            left.permission
                .as_str()
                .cmp(right.permission.as_str())
                .then_with(|| left.grant_id.cmp(&right.grant_id))
        });
        sorted
    }
    sorted(left) == sorted(right)
}

fn invocation_scope(invocation: &ContextOwnerInvocationV2) -> Scope {
    let scope = &invocation.identity.console_scope;
    Scope {
        project_id: scope.project_id.clone(),
        run_id: scope.run_id.clone(),
        episode_id: scope.episode_id.clone(),
        agent_id: scope.agent_id.clone(),
    }
}

fn validate_principal_for_invocation(
    principal: &VerifiedPrincipal,
    invocation: &ContextOwnerInvocationV2,
    now: u64,
) -> Result<(), AuthenticatedIngressError> {
    let identity = &invocation.identity.console;
    ensure_principal_current(principal, now)?;
    if principal.issuer() != identity.issuer
        || principal.subject() != identity.subject
        || principal.audience() != identity.audience
        || principal.credential_id() != identity.credential_id
    {
        return Err(AuthenticatedIngressError::Unauthorized);
    }
    Ok(())
}

fn ensure_principal_current(
    principal: &VerifiedPrincipal,
    now: u64,
) -> Result<(), AuthenticatedIngressError> {
    if principal.expires_at() > now {
        Ok(())
    } else {
        Err(AuthenticatedIngressError::Unauthorized)
    }
}

fn trusted_unix_seconds() -> Result<u64, AuthenticatedIngressError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AuthenticatedIngressError::Unauthorized)?;
    // Round toward the future so integer expiry checks and the monotonic deadline never gain
    // authority from dropping the fractional part of the current second.
    elapsed
        .as_secs()
        .checked_add(u64::from(elapsed.subsec_nanos() != 0))
        .ok_or(AuthenticatedIngressError::Unauthorized)
}

fn advance_wall_clock(floor: &mut u64) -> Result<u64, AuthenticatedIngressError> {
    *floor = (*floor).max(trusted_unix_seconds()?);
    Ok(*floor)
}

fn observe_wall_clock(floor: &Cell<u64>) -> Result<u64, AuthenticatedIngressError> {
    let now = trusted_unix_seconds()?.max(floor.get());
    floor.set(now);
    Ok(now)
}

fn map_verifier_error(error: PrincipalVerificationError) -> AuthenticatedIngressError {
    match error {
        PrincipalVerificationError::Invalid => AuthenticatedIngressError::InvalidBearer,
        PrincipalVerificationError::Unavailable => AuthenticatedIngressError::VerifierUnavailable,
    }
}

fn map_admission_error(error: AdmissionError) -> AuthenticatedIngressError {
    match error {
        AdmissionError::Denied => AuthenticatedIngressError::GrantDenied,
        AdmissionError::UnsupportedOperation => AuthenticatedIngressError::InvalidHttpRequest,
    }
}

#[cfg(test)]
#[path = "owner_invocation/tests.rs"]
mod tests;
