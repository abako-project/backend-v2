-- Private descriptive metadata remains adapter-owned, outside provider matching.
ALTER TABLE client_profiles ADD COLUMN IF NOT EXISTS contact_email TEXT;
ALTER TABLE worker_profiles ADD COLUMN IF NOT EXISTS role_experiences JSONB NOT NULL DEFAULT '[]'
    CHECK (jsonb_typeof(role_experiences) = 'array' AND jsonb_array_length(role_experiences) <= 64);
