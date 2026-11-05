-- A new authenticator secret waiting for its first code (replacing the app while two-factor stays on).
-- The `.rs` runs this only when the column is missing.
ALTER TABLE "user_totp" ADD COLUMN "pending_secret" TEXT;
