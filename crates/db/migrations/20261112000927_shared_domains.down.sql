-- Shared domains can't exist under the old schema. Emails sent from them keep their row with domain_id nulled.
delete from courier.domains where scope = 'shared';

alter table courier.emails drop constraint emails_domain_id_fkey;
-- The composite key requires each email's domain to be its project's, which nothing enforced while it was gone.
update courier.emails e
set domain_id = null
from courier.domains d
where e.domain_id = d.id and d.project_id <> e.project_id;
alter table courier.domains add constraint domains_project_id_id_key unique (project_id, id);
alter table courier.emails
    add constraint emails_project_id_domain_id_fkey foreign key (project_id, domain_id)
        references courier.domains (project_id, id) on delete set null (domain_id);

alter table courier.domains
    drop constraint domains_scope_owner,
    drop column scope,
    alter column project_id set not null;
drop type courier.domain_scope;
