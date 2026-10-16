variable "region" {
  type    = string
  default = "us-east-1"
}

variable "function_name" {
  type    = string
  default = "bedrock-api"

  validation {
    condition     = startswith(var.function_name, "bedrock-")
    error_message = "The deploy role can only manage functions named bedrock-*."
  }
}

variable "lambda_zip" {
  description = "Path to the zip produced by `cargo lambda build --release --arm64 --bin api --output-format zip`."
  type        = string
  default     = "../target/lambda/api/bootstrap.zip"
}

variable "ses_identities" {
  description = "Already-verified SES identities (domains or email addresses) the Lambda may send from. Empty means no SES access."
  type        = list(string)
  default     = []
}

variable "lambda_boundary_name" {
  description = "Permissions boundary created by infra/bootstrap. The deploy role can only create roles that carry it."
  type        = string
  default     = "bedrock-lambda-boundary"
}
