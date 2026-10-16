variable "region" {
  type    = string
  default = "us-east-1"
}

variable "github_repo" {
  description = "owner/name of the repository allowed to deploy."
  type        = string
  default     = "Hack4Impact-UMD/bedrock"
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
