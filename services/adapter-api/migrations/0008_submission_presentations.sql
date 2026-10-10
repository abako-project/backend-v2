CREATE TABLE IF NOT EXISTS submission_presentations (
    provider_instance_id TEXT NOT NULL,
    submission_id TEXT NOT NULL,
    documentation TEXT,
    links TEXT,
    PRIMARY KEY (provider_instance_id, submission_id)
);
