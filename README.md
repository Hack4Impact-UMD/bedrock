# bedrock

Shared services for H4I projects.

## Deployment

(`.github/workflows/deploy.yml`) runs on every push to `main`: it re-runs CI, builds `api` with cargo-lambda, and `terraform apply`s `infra/` (Lambda + API Gateway HTTP API in `us-east-2`).

### Bootstraping AWS setup

These steps must only be done once to setup AWS infrastructure on a new account.

1. With admin AWS credentials, create the state bucket, GitHub OIDC provider, and deploy role:
   ```sh
   terraform -chdir=infra/bootstrap init
   terraform -chdir=infra/bootstrap apply
   ```
   If the account already has a GitHub OIDC provider, pass `-var create_oidc_provider=false`.
   Bootstrap state is stored locally in `infra/bootstrap/terraform.tfstate` (gitignored), so keep it somewhere safe.
2. In GitHub → Settings → Environments, create a `production` environment (restrict it to `main`). Then add an
   environment **variable** `AWS_DEPLOY_ROLE_ARN` set to the `deploy_role_arn` output.
3. (Optional) Add SES identities that are already verified in `us-east-2` to `ses_identities` in `infra/terraform.tfvars`.
   A single email address works for testing (`aws sesv2 create-email-identity --email-identity you@example.com`, then
   click the confirmation link). While SES is in sandbox mode, it can only send to verified recipients.

The Lambda may call `ses:SendEmail`/`ses:SendRawEmail` for the listed identities only. When the list is empty, it has
no SES access.
