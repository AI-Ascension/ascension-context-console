// SPDX-License-Identifier: MIT

//! Revalidation of the live Console identity and protected owner slot for one sealed admission.

use crate::authenticated_ingress::{PrincipalVerifier, validation};

use super::{
    AuthenticatedIngress, AuthenticatedIngressError, AuthenticatedOwnerInvocation,
    VerifiedPrincipal,
};
use crate::harness_context_owner_wire::ContextOwnerInvocationV2;
use crate::harness_facade::HarnessFacadeConfig;
use crate::http::HttpRequest;
use crate::protected_owner_credentials::ProtectedOwnerCredentialResolver;
use crate::subject_grants::SubjectGrantStore;

#[cfg(test)]
#[path = "current_identity/tests.rs"]
mod tests;

impl<V, G> AuthenticatedIngress<V, G>
where
    V: PrincipalVerifier,
    G: SubjectGrantStore,
{
    /// Recheck the current request identity and exact protected credential without changing the
    /// original admission, its principal, its grant snapshot, or its monotonic deadline.
    pub(crate) fn revalidate_current_owner_identity<R: ProtectedOwnerCredentialResolver>(
        &mut self,
        request: &HttpRequest,
        facade: &HarnessFacadeConfig,
        admitted: &AuthenticatedOwnerInvocation,
        resolver: &mut R,
    ) -> Result<(), AuthenticatedIngressError> {
        let invocation = &admitted.invocation;
        invocation
            .validate()
            .map_err(|_| AuthenticatedIngressError::InvalidHttpRequest)?;
        facade
            .validate()
            .map_err(|_| AuthenticatedIngressError::InvalidConfiguration)?;
        if facade.scope != admitted.scope || super::invocation_scope(invocation) != admitted.scope {
            return Err(AuthenticatedIngressError::Unauthorized);
        }

        let (required, required_scopes) = super::selected_policy(invocation, admitted.use_kind)?;
        validation::validate_owner_http_envelope(request, facade, &required)?;
        let live_invocation: ContextOwnerInvocationV2 =
            crate::harness_context_owner_wire::decode_bounded_json(&request.body)
                .map_err(|_| AuthenticatedIngressError::InvalidHttpRequest)?;
        live_invocation
            .validate()
            .map_err(|_| AuthenticatedIngressError::InvalidHttpRequest)?;
        if live_invocation != *invocation {
            return Err(AuthenticatedIngressError::InvalidHttpRequest);
        }

        let mut now = super::observe_wall_clock(&admitted.wall_clock_floor)?;
        admitted
            .admission
            .validate_for(invocation, admitted.use_kind, now)
            .map_err(super::map_admission_error)?;
        super::validate_principal_for_invocation(&admitted.principal, invocation, now)?;
        super::validate_selected_grants(
            &admitted.grants,
            &admitted.principal,
            &admitted.scope,
            &required,
            &required_scopes,
            now,
        )?;
        validation::validate_owner_credential(
            &admitted.credential,
            &admitted.principal,
            &admitted.scope,
            &required,
            now,
        )?;

        let bearer = validation::bearer_from_headers(request)?;
        super::observe_wall_clock(&admitted.wall_clock_floor)?;
        let verified = self.verifier.verify(bearer);
        now = super::observe_wall_clock(&admitted.wall_clock_floor)?;
        let claims = verified.map_err(super::map_verifier_error)?;
        let principal = self.validate_claims(claims, now)?;
        require_same_principal(&principal, &admitted.principal)?;
        super::validate_principal_for_invocation(&principal, invocation, now)?;
        admitted
            .admission
            .validate_for(invocation, admitted.use_kind, now)
            .map_err(super::map_admission_error)?;

        let resolution = resolver.resolve(&principal, &admitted.scope, &required_scopes, now);
        now = super::observe_wall_clock(&admitted.wall_clock_floor)?;
        let resolved = resolution.map_err(validation::map_credential_error)?;
        validation::validate_owner_credential(
            &resolved,
            &principal,
            &admitted.scope,
            &required,
            now,
        )?;
        if resolved != admitted.credential {
            return Err(AuthenticatedIngressError::OwnerCredentialDenied);
        }

        // Re-query after resolver work. The existing method compares the refreshed grant rows
        // with the original sealed snapshot and checks the same admission deadline again.
        self.revalidate_owner_invocation(admitted)
    }
}

fn require_same_principal(
    current: &VerifiedPrincipal,
    original: &VerifiedPrincipal,
) -> Result<(), AuthenticatedIngressError> {
    if current.issuer() == original.issuer()
        && current.subject() == original.subject()
        && current.audience() == original.audience()
        && current.credential_id() == original.credential_id()
        && current.expires_at() == original.expires_at()
    {
        Ok(())
    } else {
        Err(AuthenticatedIngressError::Unauthorized)
    }
}
