output "deploy_role_arn" {
  description = "Set as the AWS_DEPLOY_ROLE_ARN variable on the GitHub production environment."
  value       = aws_iam_role.deploy.arn
}

output "state_bucket" {
  value = aws_s3_bucket.state.bucket
}

output "lambda_boundary_arn" {
  value = aws_iam_policy.lambda_boundary.arn
}
