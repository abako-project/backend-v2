CREATE TABLE IF NOT EXISTS proposal_presentations (
    provider_instance_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    proposal_id TEXT NOT NULL,
    revision BIGINT NOT NULL CHECK (revision > 0),
    proposal_revision BIGINT NOT NULL CHECK (proposal_revision >= 0),
    definition JSONB NOT NULL,
    milestones JSONB NOT NULL,
    PRIMARY KEY (provider_instance_id, project_id, proposal_id, revision)
);
CREATE TABLE IF NOT EXISTS proposal_review_comments (
    comment_id BIGSERIAL PRIMARY KEY,
    provider_instance_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    proposal_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    author_account TEXT NOT NULL,
    proposal_revision BIGINT NOT NULL CHECK (proposal_revision >= 0),
    created_at BIGINT NOT NULL CHECK (created_at >= 0),
    message TEXT NOT NULL CHECK (octet_length(message) BETWEEN 1 AND 10000),
    definition JSONB NOT NULL,
    presentation JSONB,
    UNIQUE (provider_instance_id, project_id, proposal_id, author_account, request_id)
);
CREATE INDEX IF NOT EXISTS proposal_review_comments_page ON proposal_review_comments
    (provider_instance_id, project_id, proposal_id, comment_id);
