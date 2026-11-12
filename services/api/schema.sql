-- Current schema for a fresh installation. Startup never upgrades or resets old data.
CREATE TABLE adapter_schema (format TEXT PRIMARY KEY CHECK(format='current'));
INSERT INTO adapter_schema VALUES ('current');

CREATE TABLE principals (
    principal_id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    is_admin BOOLEAN NOT NULL,
    wallet_id TEXT UNIQUE,
    account_id TEXT UNIQUE,
    created_at BIGINT NOT NULL,
    passkey_user_id TEXT UNIQUE
);

CREATE TABLE sessions (
    token_hash BYTEA PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    principal_id TEXT NOT NULL REFERENCES principals(principal_id),
    csrf_token TEXT NOT NULL,
    expires_at BIGINT NOT NULL
);

CREATE TABLE operations (
    creation_sequence BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    operation_id TEXT NOT NULL UNIQUE,
    principal_id TEXT NOT NULL REFERENCES principals(principal_id),
    wallet_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    command_json TEXT NOT NULL,
    command_bytes BYTEA NOT NULL,
    status TEXT NOT NULL,
    signable_payload BYTEA,
    signed_json TEXT,
    provider_instance_id TEXT,
    receipt_json TEXT,
    error_code TEXT,
    possibly_submitted BOOLEAN NOT NULL DEFAULT FALSE,
    attempt_count BIGINT NOT NULL DEFAULT 0,
    next_attempt_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    lease_token TEXT,
    lease_until BIGINT,
    created_at BIGINT NOT NULL,
    CHECK ((lease_token IS NULL) = (lease_until IS NULL)),
    request_command_bytes BYTEA NOT NULL,
    catalog_operation BOOLEAN NOT NULL DEFAULT FALSE,
    task_operation BOOLEAN NOT NULL DEFAULT FALSE,
    task_result_json TEXT,
    task_project_id TEXT,
    CONSTRAINT operations_task_source CHECK (
            (NOT task_operation
                AND status IN ('AwaitingSignature','ReadyToSubmit','Submitted','OutcomeUnknown','Finalized','Rejected','Expired')
                AND task_result_json IS NULL)
            OR (task_operation
                AND status IN ('Queued','Finalized','Rejected','Expired')
                AND provider_instance_id IS NOT NULL
                AND signable_payload IS NULL AND signed_json IS NULL AND receipt_json IS NULL
                AND NOT possibly_submitted AND NOT catalog_operation
                AND ((status='Finalized') = (task_result_json IS NOT NULL)))
        ),
    CONSTRAINT operations_task_project_source CHECK (
    task_project_id IS NULL OR (
        provider_instance_id IS NOT NULL
        AND request_command_bytes IS NOT NULL
        AND NOT catalog_operation
    )
)
);

CREATE TABLE event_cursors (
    provider_instance_id TEXT PRIMARY KEY,
    cursor BIGINT NOT NULL CHECK (cursor >= 0)
);

CREATE TABLE notifications (
    notification_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id TEXT NOT NULL,
    provider_instance_id TEXT NOT NULL,
    provider_cursor BIGINT,
    event_json TEXT NOT NULL,
    read_at BIGINT,
    UNIQUE (account_id, provider_instance_id, provider_cursor),
    api_operation_id TEXT REFERENCES operations(operation_id),
    CONSTRAINT notifications_event_source CHECK (
            (provider_cursor IS NOT NULL AND api_operation_id IS NULL)
            OR (provider_cursor IS NULL AND api_operation_id IS NOT NULL)
        )
);

CREATE TABLE audit_records (
    audit_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    principal_id TEXT,
    operation_id TEXT,
    event_kind TEXT NOT NULL,
    result_code TEXT NOT NULL,
    occurred_at BIGINT NOT NULL
);

CREATE TABLE client_profiles (
    principal_id TEXT PRIMARY KEY REFERENCES principals(principal_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    company TEXT,
    department TEXT,
    website TEXT,
    description TEXT,
    location TEXT,
    languages TEXT[] NOT NULL DEFAULT '{}',
    updated_at BIGINT NOT NULL,
    image_data BYTEA,
    image_mime_type TEXT,
    contact_email TEXT
);

CREATE TABLE worker_profiles (
    principal_id TEXT PRIMARY KEY REFERENCES principals(principal_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    github_username TEXT,
    portfolio_url TEXT,
    biography TEXT,
    background TEXT,
    proficiency TEXT CHECK (proficiency IN ('junior', 'mid-level', 'senior')),
    location TEXT,
    languages TEXT[] NOT NULL DEFAULT '{}',
    updated_at BIGINT NOT NULL,
    image_data BYTEA,
    image_mime_type TEXT,
    contact_email TEXT,
    role_experiences JSONB NOT NULL DEFAULT '[]'
    CHECK (jsonb_typeof(role_experiences) = 'array' AND jsonb_array_length(role_experiences) <= 64)
);

CREATE TABLE passkey_credentials (
    credential_ref TEXT PRIMARY KEY,
    credential_id BYTEA NOT NULL UNIQUE,
    principal_id TEXT NOT NULL REFERENCES principals(principal_id) ON DELETE CASCADE,
    passkey_json TEXT NOT NULL,
    sign_count BIGINT NOT NULL DEFAULT 0 CHECK (sign_count >= 0),
    created_at BIGINT NOT NULL
);

CREATE TABLE passkey_ceremonies (
    ceremony_id TEXT PRIMARY KEY,
    principal_id TEXT NOT NULL REFERENCES principals(principal_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('register', 'login')),
    state_json TEXT NOT NULL,
    session_token_hash BYTEA CHECK (session_token_hash IS NULL OR octet_length(session_token_hash) = 32),
    expires_at BIGINT NOT NULL,
    CHECK ((kind = 'register') = (session_token_hash IS NOT NULL))
);

CREATE TABLE project_briefs (
    provider_instance_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    revision BIGINT NOT NULL CHECK (revision > 0),
    brief JSONB NOT NULL,
    PRIMARY KEY (provider_instance_id, project_id)
);

CREATE TABLE proposal_presentations (
    provider_instance_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    proposal_id TEXT NOT NULL,
    revision BIGINT NOT NULL CHECK (revision > 0),
    proposal_revision BIGINT NOT NULL CHECK (proposal_revision >= 0),
    definition JSONB NOT NULL,
    milestones JSONB NOT NULL,
    PRIMARY KEY (provider_instance_id, project_id, proposal_id, revision)
);

CREATE TABLE proposal_review_comments (
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
    UNIQUE (provider_instance_id, project_id, proposal_id, author_account, request_id),
    delivery JSONB
);

CREATE TABLE submission_presentations (
    provider_instance_id TEXT NOT NULL,
    submission_id TEXT NOT NULL,
    documentation TEXT,
    links TEXT,
    PRIMARY KEY (provider_instance_id, submission_id)
);

CREATE TABLE submission_comments (
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

CREATE TABLE dispute_opening_arguments (
    provider_instance_id TEXT NOT NULL,
    comment_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    milestone_id TEXT NOT NULL,
    submission_id TEXT NOT NULL,
    author_account TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (octet_length(reason) BETWEEN 1 AND 4000),
    PRIMARY KEY (provider_instance_id, comment_id)
);

CREATE TABLE dispute_entries (
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

CREATE TABLE evidence_url_records (
    provider_instance_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    purpose SMALLINT NOT NULL CHECK (purpose BETWEEN 0 AND 3),
    target_id TEXT NOT NULL,
    record_id TEXT NOT NULL,
    author_account TEXT NOT NULL,
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 8192),
    PRIMARY KEY (provider_instance_id, project_id, purpose, target_id, record_id)
);

CREATE TABLE worker_directory_metadata (
    provider_instance_id TEXT NOT NULL,
    content_hash BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 33554432),
    PRIMARY KEY (provider_instance_id, content_hash)
);

CREATE TABLE project_creation_metadata (
    provider_instance_id TEXT NOT NULL,
    content_hash BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 33554432),
    PRIMARY KEY (provider_instance_id, content_hash)
);

CREATE TABLE proposal_documents (
    provider_instance_id TEXT NOT NULL,
    content_hash BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 33554432),
    PRIMARY KEY (provider_instance_id, content_hash)
);

CREATE TABLE catalog_documents (
    provider_instance_id TEXT NOT NULL,
    content_hash BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 33554432),
    PRIMARY KEY(provider_instance_id,content_hash)
);

CREATE TABLE lifecycle_reason_documents (
    provider_instance_id TEXT NOT NULL,
    content_hash BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 33554432),
    PRIMARY KEY(provider_instance_id,content_hash)
);

CREATE TABLE catalog_skill_names (
    provider_instance_id TEXT NOT NULL,
    name_hash BYTEA NOT NULL CHECK (octet_length(name_hash) = 32),
    normalized BYTEA NOT NULL CHECK (octet_length(normalized) BETWEEN 1 AND 33554432),
    skill_id BIGINT CHECK(skill_id BETWEEN 1 AND 4294967295),
    label_hash BYTEA CHECK(octet_length(label_hash) = 32),
    origin_hash BYTEA CHECK(octet_length(origin_hash) = 32),
    reservation_op TEXT REFERENCES operations(operation_id),
    PRIMARY KEY(provider_instance_id,name_hash),
    CHECK ((skill_id IS NULL) = (label_hash IS NULL)),
    CHECK ((skill_id IS NULL) = (origin_hash IS NULL)),
    CHECK (skill_id IS NOT NULL OR reservation_op IS NOT NULL)
);

CREATE TABLE task_generations (
    provider_instance_id TEXT PRIMARY KEY,
    next_task_id BIGINT NOT NULL CHECK(next_task_id BETWEEN 1 AND 4294967295)
);

CREATE TABLE task_storages (
    provider_instance_id TEXT NOT NULL REFERENCES task_generations(provider_instance_id) ON DELETE CASCADE,
    task_storage_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    proposal_id TEXT NOT NULL,
    proposal_revision TEXT NOT NULL,
    milestone_id TEXT NOT NULL,
    PRIMARY KEY(provider_instance_id,task_storage_id),
    UNIQUE(provider_instance_id,project_id,proposal_id,milestone_id)
);

CREATE TABLE task_items (
    provider_instance_id TEXT NOT NULL,
    task_id BIGINT NOT NULL CHECK(task_id BETWEEN 1 AND 4294967295),
    task_storage_id TEXT NOT NULL,
    ordinal BIGINT NOT NULL CHECK(ordinal>=0),
    payload TEXT NOT NULL,
    assignees TEXT[] NOT NULL,
    PRIMARY KEY(provider_instance_id,task_id),
    UNIQUE(provider_instance_id,task_storage_id,ordinal),
    FOREIGN KEY(provider_instance_id,task_storage_id) REFERENCES task_storages(provider_instance_id,task_storage_id) ON DELETE CASCADE
);

CREATE TABLE task_projects (
    provider_instance_id TEXT NOT NULL REFERENCES task_generations(provider_instance_id) ON DELETE CASCADE,
    project_id TEXT NOT NULL,
    PRIMARY KEY(provider_instance_id,project_id)
);

CREATE TABLE task_freezes (
    operation_id TEXT PRIMARY KEY REFERENCES operations(operation_id),
    provider_instance_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('Cancellation','Dispute')),
    state TEXT NOT NULL DEFAULT 'Pending' CHECK(state IN ('Pending','Confirmed','Released')),
    snapshot_json TEXT NOT NULL,
    FOREIGN KEY(provider_instance_id,project_id) REFERENCES task_projects(provider_instance_id,project_id)
);

CREATE INDEX sessions_principal ON sessions(principal_id);
CREATE UNIQUE INDEX operations_wallet_lease ON operations(wallet_id) WHERE lease_token IS NOT NULL;
CREATE INDEX operations_queue ON operations(status, next_attempt_at, creation_sequence);
CREATE INDEX notifications_account ON notifications(account_id, notification_id);
CREATE INDEX passkeys_principal ON passkey_credentials(principal_id);
CREATE INDEX passkey_ceremonies_expiry ON passkey_ceremonies(expires_at);
CREATE INDEX proposal_review_comments_page ON proposal_review_comments
    (provider_instance_id, project_id, proposal_id, comment_id);
CREATE INDEX dispute_entries_page ON dispute_entries(provider_instance_id, dispute_id, cursor);
CREATE INDEX operations_catalog_sequence ON operations(creation_sequence) WHERE catalog_operation;
CREATE UNIQUE INDEX catalog_skill_name_identity ON catalog_skill_names(provider_instance_id,skill_id) WHERE skill_id IS NOT NULL;
CREATE UNIQUE INDEX catalog_skill_name_reservation ON catalog_skill_names(provider_instance_id,reservation_op) WHERE reservation_op IS NOT NULL;
CREATE UNIQUE INDEX notifications_api_operation
    ON notifications(account_id,provider_instance_id,api_operation_id) WHERE api_operation_id IS NOT NULL;
CREATE INDEX task_storages_project ON task_storages(provider_instance_id,project_id);
CREATE INDEX operations_task_generation ON operations(provider_instance_id) WHERE task_operation;
CREATE INDEX task_freezes_project ON task_freezes(provider_instance_id,project_id) WHERE state<>'Released';
CREATE INDEX operations_task_project_order
    ON operations(provider_instance_id,task_project_id,creation_sequence)
    WHERE task_project_id IS NOT NULL;
