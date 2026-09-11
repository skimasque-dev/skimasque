# A minimal AWS reference deployment for the skimasque gateway: one instance
# running the container, a security group that admits QUIC (UDP) from your CI
# egress, and an Elastic IP so the address is stable.
#
# This is a starting point, not a production module. For real use you will want
# an autoscaling group behind a UDP-capable NLB, the image from your own
# registry, secrets from SSM/Secrets Manager, and the policy files delivered by
# your config management. See deploy/helm for the Kubernetes path.

terraform {
  required_version = ">= 1.5"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.0"
    }
  }
}

variable "name" {
  type    = string
  default = "skimasque-gateway"
}

variable "vpc_id" {
  type = string
}

variable "subnet_id" {
  description = "A public subnet; the gateway needs an outbound path to the destinations it proxies."
  type        = string
}

variable "instance_type" {
  type    = string
  default = "t4g.small" # arm64
}

variable "image" {
  description = "The gateway container image."
  type        = string
  default     = "ghcr.io/skimasque-dev/skimasque:latest"
}

variable "authority" {
  description = "The gateway's TLS name / :authority."
  type        = string
}

variable "ci_egress_cidrs" {
  description = "CIDRs allowed to open QUIC connections (your CI runners' egress IPs)."
  type        = list(string)
}

variable "extra_server_args" {
  description = "Appended to `skimasque-server` (auth, policy, OIDC, ...)."
  type        = string
  default     = "--metrics-listen 127.0.0.1:9090"
}

data "aws_ami" "al2023_arm64" {
  most_recent = true
  owners      = ["amazon"]
  filter {
    name   = "name"
    values = ["al2023-ami-*-arm64"]
  }
}

resource "aws_security_group" "gateway" {
  name_prefix = "${var.name}-"
  vpc_id      = var.vpc_id

  ingress {
    description = "MASQUE / QUIC"
    from_port   = 4433
    to_port     = 4433
    protocol    = "udp"
    cidr_blocks = var.ci_egress_cidrs
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  lifecycle {
    create_before_destroy = true
  }
}

locals {
  user_data = <<-EOT
    #!/bin/bash
    set -euo pipefail
    dnf install -y docker
    systemctl enable --now docker
    docker run -d --restart=always --name skimasque-gateway \
      -p 4433:4433/udp \
      ${var.image} \
      --listen 0.0.0.0:4433 --authority ${var.authority} ${var.extra_server_args}
  EOT
}

resource "aws_instance" "gateway" {
  ami                    = data.aws_ami.al2023_arm64.id
  instance_type          = var.instance_type
  subnet_id              = var.subnet_id
  vpc_security_group_ids = [aws_security_group.gateway.id]
  user_data              = local.user_data

  metadata_options {
    http_tokens = "required" # IMDSv2 only
  }

  tags = { Name = var.name }
}

resource "aws_eip" "gateway" {
  instance = aws_instance.gateway.id
  domain   = "vpc"
  tags     = { Name = var.name }
}

output "gateway_address" {
  description = "Point clients here as <address>:4433"
  value       = aws_eip.gateway.public_ip
}
