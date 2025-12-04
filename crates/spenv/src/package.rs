// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! SPK package resolution helpers for spenv (feature-gated).

#![cfg(feature = "spk")]

use crate::spec::PackageOptions;
use crate::Error;
use spk_exec::setup_runtime_with_reporter;
use spk_solve::{DecisionFormatterBuilder, Request as SolveRequest, SolverExt, SolverImpl, SolverMut, ResolvoSolver, StepSolver};
use spk_solve::solution::Solution;
use spk_solve::{PkgRequest, RequestedBy};
use spk_storage as storage;
use spk_schema::ident::parse_ident;


/// Resolve package requests into a SPK solution using the configured solver
/// and repositories.
pub async fn resolve_packages(
    packages: &[String],
    options: &PackageOptions,
    repos: &[std::sync::Arc<storage::RepositoryHandle>],
) -> crate::Result<Solution> {
    if packages.is_empty() {
        return Err(Error::ValidationFailed(
            "No packages specified in packages:[]".to_string(),
        ));
    }

    // Choose solver implementation.
    let mut solver = match options.solver.as_deref() {
        Some("resolvo") => SolverImpl::Resolvo(ResolvoSolver::default()),
        Some("step") | None => SolverImpl::Step(StepSolver::default()),
        Some(other) => {
            return Err(Error::ValidationFailed(format!(
                "Unknown solver: {} (expected 'step' or 'resolvo')",
                other
            )));
        }
    };

    solver.set_binary_only(options.binary_only);

    // Configure repositories: for now, defer to provided repos slice, which
    // should be derived from the runtime config.
    for repo in repos {
        solver.add_repository(repo.clone());
    }

    // Parse package requests.
    for pkg in packages {
        let ident = parse_ident(pkg)
            .map_err(|e| Error::ValidationFailed(format!("Invalid package request '{}': {}", pkg, e)))?;
        let pkg_req = PkgRequest::from_ident(ident, RequestedBy::DoesNotMatter);
        solver.add_request(SolveRequest::Pkg(pkg_req));
    }

    // Solve and return the solution.
    let formatter = DecisionFormatterBuilder::default().build();
    let solution = solver
        .run_and_print_resolve(&formatter)
        .await
        .map_err(|e| Error::ValidationFailed(format!("Package resolution failed: {}", e)))?;
    Ok(solution)
}

/// Apply a SPK solution to a runtime by resolving layers and updating the
/// runtime stack using spk-exec.
pub async fn apply_solution_to_runtime(
    runtime: &mut spfs::runtime::Runtime,
    solution: &Solution,
) -> crate::Result<()> {
    setup_runtime_with_reporter(runtime, solution, spfs::sync::reporter::SyncReporters::console)
        .await
        .map_err(|e| Error::ValidationFailed(format!("Failed to apply SPK solution: {}", e)))
}
