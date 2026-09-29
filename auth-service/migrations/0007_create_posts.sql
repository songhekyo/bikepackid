CREATE TYPE post_type AS ENUM ('photo', 'video', 'text', 'thread_item');
CREATE TYPE post_status AS ENUM ('published', 'flagged', 'removed');

CREATE TABLE posts (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    checkpoint_id  UUID NOT NULL REFERENCES checkpoints(id) ON DELETE CASCADE,
    type           post_type NOT NULL,
    body           TEXT,
    media_url      TEXT,
    parent_post_id UUID REFERENCES posts(id) ON DELETE CASCADE,
    status         post_status NOT NULL DEFAULT 'published',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX posts_checkpoint_id_idx ON posts (checkpoint_id);
