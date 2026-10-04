-- Early-access waitlist for the landing page (web/index.html's `/api/waitlist`
-- form) — a plain email collection, not tied to a `users` row at all. People
-- here haven't authenticated with Google yet; onboarding them into `users`
-- with a real role is a manual step once there's capacity, not automatic.
CREATE TABLE waitlist_signups (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email      TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
