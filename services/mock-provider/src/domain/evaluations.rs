use super::{State, add_rating, add_weighted_rating};
use crate::{Error, Result, require};
use generated_contracts::{
    AccountId32, EvaluateProjectRequest, Minutes, ProjectEvaluation, ProjectView, ProposalStatus,
    Score, UnixSeconds,
};
use std::collections::{BTreeMap, BTreeSet};

impl State {
    pub(super) fn evaluate_project(
        &mut self,
        project: &mut ProjectView,
        origin: AccountId32,
        request: &EvaluateProjectRequest,
        now: UnixSeconds,
    ) -> Result<()> {
        require(
            project.completed && !project.cancelled && project.active_dispute_id.is_none(),
            "project_not_completed",
        )?;
        require(
            !project.evaluations.iter().any(|e| e.author == origin),
            "project_already_evaluated",
        )?;
        let mut minutes: BTreeMap<AccountId32, Minutes> = BTreeMap::new();
        let mut coordinator_minutes = Minutes::ZERO;
        for milestone in project
            .proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Approved)
            .flat_map(|p| &p.milestones)
        {
            coordinator_minutes =
                coordinator_minutes.checked_add(milestone.definition.coordinator_minutes)?;
            for requirement in &milestone.definition.requirements {
                let worker = milestone
                    .assignments
                    .iter()
                    .find(|a| a.requirement_key == requirement.key)
                    .ok_or_else(Error::internal)?
                    .worker;
                let prior = minutes.entry(worker).or_default();
                *prior = prior.checked_add(requirement.minutes)?;
            }
        }
        let mut expected: BTreeSet<_> = if origin == project.coordinator {
            minutes.keys().copied().collect()
        } else {
            require(
                origin == project.client || minutes.contains_key(&origin),
                "project_participant_required",
            )?;
            BTreeSet::new()
        };
        expected.insert(if origin == project.coordinator {
            project.client
        } else {
            project.coordinator
        });
        expected.remove(&origin);
        let actual: BTreeSet<_> = request.ratings.iter().map(|r| r.account).collect();
        require(
            expected == actual && actual.len() == request.ratings.len(),
            "evaluation_participants_mismatch",
        )?;
        project.evaluations.push(ProjectEvaluation {
            author: origin,
            submitted_at: now,
            ratings: request.ratings.clone(),
        });
        let score = |author: AccountId32, target: AccountId32| -> Result<Score> {
            project
                .evaluations
                .iter()
                .find(|e| e.author == author)
                .and_then(|e| e.ratings.iter().find(|r| r.account == target))
                .map(|r| r.score)
                .ok_or_else(Error::internal)
        };
        if origin != project.coordinator {
            let weight = if origin == project.client {
                self.score_policy.coordinator_client_vote_weight()
            } else {
                self.score_policy.coordinator_worker_vote_weight()
            };
            add_weighted_rating(
                &mut self.worker_mut(project.coordinator)?.coordinator_score,
                u16::from(score(origin, project.coordinator)?.get()) * 100,
                coordinator_minutes,
                weight,
            )?;
        }
        if origin == project.coordinator {
            for (worker, minutes) in minutes {
                if worker == project.coordinator {
                    continue;
                }
                let value = u16::from(score(origin, worker)?.get()) * 100;
                add_rating(&mut self.worker_mut(worker)?.worker_score, value, minutes)?;
            }
        }
        Ok(())
    }
}
