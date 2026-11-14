output "api_endpoint" {
  value = aws_apigatewayv2_stage.default.invoke_url
}

output "function_name" {
  value = aws_lambda_function.api.function_name
}

output "api_url" {
  value = "https://${aws_apigatewayv2_domain_name.api.domain_name}"
}

# CNAME target for var.api_domain in the dns repo (dnsconfig.js).
output "api_domain_target" {
  value = aws_apigatewayv2_domain_name.api.domain_name_configuration[0].target_domain_name
}
