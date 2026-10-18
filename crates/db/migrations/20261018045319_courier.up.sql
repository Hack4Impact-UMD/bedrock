create schema courier;

create type courier.domain_status as enum ('pending', 'verified', 'failed', 'temporary_failure');

create table courier.domains (
    id              uuid primary key default gen_random_uuid(),
    project_id      uuid not null references identity.projects (id) on delete cascade,
    domain          text not null,
    status          courier.domain_status not null default 'pending',
    -- [{type, name, value}] returned by SES CreateEmailIdentity.
    dns_records     jsonb not null,
    created_at      timestamptz not null default now(),
    last_checked_at timestamptz,
    verified_at     timestamptz
);

-- SES identities are global to the AWS account, so a domain can belong to only one project.
create unique index domains_domain_key on courier.domains (lower(domain));
create index domains_project_id_idx on courier.domains (project_id);

create type courier.send_mode as enum ('live', 'test');
create type courier.email_status as enum ('sent', 'failed');

-- One row per send. Bodies are not stored.
create table courier.emails (
    id                  uuid primary key default gen_random_uuid(),
    project_id          uuid not null references identity.projects (id) on delete cascade,
    api_key_id          uuid references identity.api_keys (id) on delete set null,
    domain_id           uuid references courier.domains (id) on delete set null,
    mode                courier.send_mode not null,
    from_address        text not null,
    to_addresses        text[] not null,
    cc_addresses        text[] not null default '{}',
    bcc_addresses       text[] not null default '{}',
    subject             text not null,
    status              courier.email_status not null,
    provider_message_id text,
    error               text,
    created_at          timestamptz not null default now()
);

create index emails_project_id_created_at_idx on courier.emails (project_id, created_at desc, id);
