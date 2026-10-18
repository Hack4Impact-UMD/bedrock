data "aws_caller_identity" "current" {}

locals {
  account_id = data.aws_caller_identity.current.account_id
}

# --- Execution role -----------------------------------------------------------

data "aws_iam_policy_document" "lambda_assume" {
  statement {
    actions = ["sts:AssumeRole"]

    principals {
      type        = "Service"
      identifiers = ["lambda.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "lambda_exec" {
  # The deploy role may only manage roles under the bedrock-lambda- prefix.
  name                 = "bedrock-lambda-${trimprefix(var.function_name, "bedrock-")}"
  assume_role_policy   = data.aws_iam_policy_document.lambda_assume.json
  permissions_boundary = "arn:aws:iam::${local.account_id}:policy/${var.lambda_boundary_name}"
}

resource "aws_iam_role_policy_attachment" "lambda_logs" {
  role       = aws_iam_role.lambda_exec.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

data "aws_iam_policy_document" "lambda_ses" {
  count = length(var.ses_identities) == 0 ? 0 : 1

  statement {
    actions   = ["ses:SendEmail", "ses:SendRawEmail"]
    resources = [for id in var.ses_identities : "arn:aws:ses:${var.region}:${local.account_id}:identity/${id}"]
  }
}

resource "aws_iam_role_policy" "lambda_ses" {
  count = length(var.ses_identities) == 0 ? 0 : 1

  name   = "ses-send"
  role   = aws_iam_role.lambda_exec.id
  policy = data.aws_iam_policy_document.lambda_ses[0].json
}

# --- Function -----------------------------------------------------------------

resource "aws_cloudwatch_log_group" "api" {
  name              = "/aws/lambda/${var.function_name}"
  retention_in_days = 14
}

resource "aws_lambda_function" "api" {
  function_name    = var.function_name
  role             = aws_iam_role.lambda_exec.arn
  runtime          = "provided.al2023"
  handler          = "bootstrap"
  architectures    = ["arm64"]
  filename         = var.lambda_zip
  source_code_hash = filebase64sha256(var.lambda_zip)
  memory_size      = 128
  timeout          = 10

  environment {
    variables = {
      DATABASE_URL = var.database_url
    }
  }

  depends_on = [
    aws_cloudwatch_log_group.api,
    aws_iam_role_policy_attachment.lambda_logs,
  ]
}

# --- HTTP API -----------------------------------------------------------------

resource "aws_apigatewayv2_api" "api" {
  name          = var.function_name
  protocol_type = "HTTP"
}

resource "aws_apigatewayv2_integration" "api" {
  api_id                 = aws_apigatewayv2_api.api.id
  integration_type       = "AWS_PROXY"
  integration_uri        = aws_lambda_function.api.invoke_arn
  payload_format_version = "2.0"
}

resource "aws_apigatewayv2_route" "default" {
  api_id    = aws_apigatewayv2_api.api.id
  route_key = "$default"
  target    = "integrations/${aws_apigatewayv2_integration.api.id}"
}

resource "aws_apigatewayv2_stage" "default" {
  api_id      = aws_apigatewayv2_api.api.id
  name        = "$default"
  auto_deploy = true
}

resource "aws_lambda_permission" "apigw" {
  statement_id  = "AllowAPIGatewayInvoke"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.api.function_name
  principal     = "apigateway.amazonaws.com"
  source_arn    = "${aws_apigatewayv2_api.api.execution_arn}/*"
}
