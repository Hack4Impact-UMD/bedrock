-- Local development data. Safe to run repeatedly.
-- API key: bdrk_localdev00000000000000000000000000000000

insert into identity.projects (slug, name)
values ('demo', 'Demo')
on conflict (slug) do nothing;

insert into identity.api_keys (project_id, name, prefix, hash)
select id, 'local dev', 'bdrk_localdev', sha256(convert_to('bdrk_localdev00000000000000000000000000000000', 'UTF8'))
from identity.projects
where slug = 'demo'
on conflict (prefix) do nothing;
