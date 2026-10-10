CREATE TABLE IF NOT EXISTS submission_comments (
 cursor BIGSERIAL UNIQUE NOT NULL,
 provider_instance_id TEXT NOT NULL,
 submission_id TEXT NOT NULL,
 comment_id TEXT NOT NULL,
 author_account TEXT NOT NULL,
 created_at BIGINT NOT NULL,
 message TEXT NOT NULL CHECK (octet_length(message) BETWEEN 1 AND 4000),
 kind TEXT NOT NULL CHECK (kind IN ('Comment','Rejection')),
 PRIMARY KEY (provider_instance_id, submission_id, comment_id)
);
