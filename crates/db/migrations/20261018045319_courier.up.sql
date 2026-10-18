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
    verified_at     timestamptz,
    -- Target for project-scoped foreign keys from courier.emails.
    unique (project_id, id)
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
    api_key_id          uuid,
    domain_id           uuid,
    mode                courier.send_mode not null,
    from_address        text not null,
    to_addresses        text[] not null,
    cc_addresses        text[] not null default '{}',
    bcc_addresses       text[] not null default '{}',
    subject             text not null,
    status              courier.email_status not null,
    provider_message_id text,
    error               text,
    created_at          timestamptz not null default now(),
    -- The key and domain must belong to the email's project. Only the referencing column is nulled on delete,
    -- since project_id is not null.
    foreign key (project_id, api_key_id) references identity.api_keys (project_id, id)
        on delete set null (api_key_id),
    foreign key (project_id, domain_id) references courier.domains (project_id, id)
        on delete set null (domain_id)
);

create index emails_project_id_created_at_idx on courier.emails (project_id, created_at desc, id);
