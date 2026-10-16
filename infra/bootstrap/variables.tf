variable "region" {
  type    = string
  default = "us-east-2"
}

variable "github_subject_prefix" {
  description = <<-EOT
    OIDC subject prefix of the repository allowed to deploy. The repo uses immutable
    subjects (owner/repo IDs, so a renamed or recreated repo can't inherit access).
    Get it with: gh api repos/OWNER/REPO/actions/oidc/customization/sub --jq .sub_claim_prefix
  EOT
  type        = string
  default     = "repo:Hack4Impact-UMD@71531438/bedrock@1418205202"
}

variable "github_environment" {
  description = "GitHub Actions environment the deploy job runs in. Only jobs in this environment can assume the deploy role."
  type        = string
  default     = "production"
}

variable "state_bucket" {
  description = "S3 bucket for the app's Terraform state. Must match the backend block in infra/versions.tf."
  type        = string
  default     = "h4i-bedrock-tfstate"
}

variable "create_oidc_provider" {
  description = "Set to false if the AWS account already has a token.actions.githubusercontent.com OIDC provider."
  type        = bool
  default     = true
}
