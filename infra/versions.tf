terraform {
  required_version = ">= 1.11"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.0"
    }
  }

  # Created by infra/bootstrap. Backend blocks can't reference variables, so keep
  # this in sync with the bootstrap's state_bucket variable.
  backend "s3" {
    bucket       = "h4i-bedrock-tfstate"
    key          = "app/terraform.tfstate"
    region       = "us-east-1"
    use_lockfile = true
    encrypt      = true
  }
}

provider "aws" {
  region = var.region

  default_tags {
    tags = {
      project    = "bedrock"
      managed-by = "terraform"
    }
  }
}
