-- Shared domains have no owning project and can be sent from by any project. Scope is explicit so a missing
-- project_id is an error, never an accidental share.
create type courier.domain_scope as enum ('project', 'shared');

alter table courier.domains
    add column scope courier.domain_scope not null default 'project',
    alter column project_id drop not null,
    add constraint domains_scope_owner check (
        (scope = 'project' and project_id is not null) or
        (scope = 'shared' and project_id is null)
    );

-- An email's project can differ from its domain's (for shared domains), so the composite foreign key becomes a
-- plain one. The send path checks that the domain is the project's own or shared before sending.
alter table courier.emails drop constraint emails_project_id_domain_id_fkey;
alter table courier.emails
    add constraint emails_domain_id_fkey foreign key (domain_id) references courier.domains (id)
        on delete set null;
alter table courier.domains drop constraint domains_project_id_id_key;
