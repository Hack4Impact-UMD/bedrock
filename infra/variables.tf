variable "region" {
  type    = string
  default = "us-east-2"
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

variable "lambda_boundary_name" {
  description = "Permissions boundary created by infra/bootstrap. The deploy role can only create roles that carry it."
  type        = string
  default     = "bedrock-lambda-boundary"
}

variable "database_url" {
  description = "Neon pooled connection string, exposed to the Lambda as DATABASE_URL."
  type        = string
  sensitive   = true
}

variable "api_domain" {
  description = "Custom domain for the HTTP API. Needs an ISSUED ACM certificate for this exact name in var.region; DNS lives in the h4i dns repo (dnsconfig.js)."
  type        = string
  default     = "bedrock.umdh4i.org"
}

variable "throttle_rate_limit" {
  description = "Steady-state requests per second allowed across all clients, enforced by API Gateway before Lambda runs."
  type        = number
  default     = 25
}

variable "throttle_burst_limit" {
  description = "Token bucket size for API Gateway throttling: how far above the steady rate a short burst can go."
  type        = number
  default     = 50
}

variable "lambda_reserved_concurrency" {
  description = "Maximum concurrent Lambda instances. Each holds up to db::MAX_CONNECTIONS Postgres connections, so this also caps load on Neon."
  type        = number
  default     = 10
}
