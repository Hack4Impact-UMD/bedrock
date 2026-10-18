create schema identity;

create table identity.projects (
    id         uuid primary key default gen_random_uuid(),
    slug       text not null unique check (slug ~ '^[a-z0-9-]{3,40}$'),
    name       text not null,
    created_at timestamptz not null default now()
);

create table identity.api_keys (
    id           uuid primary key default gen_random_uuid(),
    project_id   uuid not null references identity.projects (id) on delete cascade,
    name         text not null,
    -- Leading characters of the key, used to look it up and to identify it in listings.
    prefix       text not null unique,
    -- SHA-256 of the full key. Keys are 32 random bytes, so a slow KDF is unnecessary.
    hash         bytea not null,
    created_at   timestamptz not null default now(),
    last_used_at timestamptz,
    revoked_at   timestamptz
);

create index api_keys_project_id_idx on identity.api_keys (project_id);
