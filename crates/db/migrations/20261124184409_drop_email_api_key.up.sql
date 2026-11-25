-- Also drops the (project_id, api_key_id) foreign key.
alter table courier.emails drop column api_key_id;
