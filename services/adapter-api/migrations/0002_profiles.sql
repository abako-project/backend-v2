-- Descriptive profiles belong to the adapter. A principal may have both sections.
-- Provider-owned qualifications, calendars and scores are deliberately absent.
CREATE TABLE IF NOT EXISTS client_profiles (
    principal_id TEXT PRIMARY KEY REFERENCES principals(principal_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    company TEXT,
    department TEXT,
    website TEXT,
    description TEXT,
    location TEXT,
    languages TEXT[] NOT NULL DEFAULT '{}',
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS worker_profiles (
    principal_id TEXT PRIMARY KEY REFERENCES principals(principal_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    github_username TEXT,
    portfolio_url TEXT,
    biography TEXT,
    background TEXT,
    proficiency TEXT CHECK (proficiency IN ('junior', 'mid-level', 'senior')),
    location TEXT,
    languages TEXT[] NOT NULL DEFAULT '{}',
    updated_at BIGINT NOT NULL
);
