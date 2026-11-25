alter table courier.emails
    add column api_key_id uuid,
    add constraint emails_project_id_api_key_id_fkey foreign key (project_id, api_key_id)
        references identity.api_keys (project_id, id) on delete set null (api_key_id);
