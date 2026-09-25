-- Adapter-owned public profile images. Existing profile rows start without an image.
ALTER TABLE client_profiles ADD COLUMN IF NOT EXISTS image_data BYTEA;
ALTER TABLE client_profiles ADD COLUMN IF NOT EXISTS image_mime_type TEXT;
ALTER TABLE worker_profiles ADD COLUMN IF NOT EXISTS image_data BYTEA;
ALTER TABLE worker_profiles ADD COLUMN IF NOT EXISTS image_mime_type TEXT;
