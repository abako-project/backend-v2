CREATE TABLE IF NOT EXISTS dispute_opening_arguments (
    provider_instance_id TEXT NOT NULL,
    comment_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    milestone_id TEXT NOT NULL,
    submission_id TEXT NOT NULL,
    author_account TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (octet_length(reason) BETWEEN 1 AND 4000),
    PRIMARY KEY (provider_instance_id, comment_id)
);
CREATE TABLE IF NOT EXISTS dispute_entries (
    cursor BIGSERIAL UNIQUE NOT NULL,
    provider_instance_id TEXT NOT NULL,
    dispute_id TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    author_account TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('RESPONSE', 'ADDITIONAL', 'MESSAGE')),
    content TEXT NOT NULL CHECK (octet_length(content) BETWEEN 1 AND 4000),
    PRIMARY KEY (provider_instance_id, dispute_id, entry_id)
);
CREATE INDEX IF NOT EXISTS dispute_entries_page ON dispute_entries(provider_instance_id, dispute_id, cursor);
