-- Status of the custom MAIL FROM domain, tracked separately because it doesn't affect whether the
-- domain can send.
alter table courier.domains
    add column mail_from_status courier.domain_status not null default 'pending';
