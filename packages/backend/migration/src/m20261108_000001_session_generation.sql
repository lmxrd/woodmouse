-- A counter raised whenever an account is disabled. It is part of the session hash (`api::sessions`), so
-- disabling someone ends their sessions for good: enabling the account again does not bring them back.
ALTER TABLE "users" ADD COLUMN "session_generation" INTEGER NOT NULL DEFAULT 0;
