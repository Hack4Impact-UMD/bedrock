-- A send whose outcome at the provider is unclear (timeout, server error), so it may still be delivered.
alter type courier.email_status add value 'unknown';
