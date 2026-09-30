# A gateway VM in an existing VPC. Copy into your stack and adjust.
#
#   terraform init && terraform apply
#
# Then: point an A record for `hostname` at module.gateway.address, grant
# module.gateway.service_account_email read access to the release bucket, and
# add the one-time registration token to the secret named by
# module.gateway.registration_token_secret.

provider "google" {
  project = "my-project"
  region  = "us-central1"
}

module "gateway" {
  source = "../.."

  name       = "acme"
  hostname   = "gateway.example.com"
  project_id = "my-project"
  zone       = "us-central1-a"
  # The default VPC ships firewall rules that open SSH/RDP to the internet; use a
  # dedicated VPC outside of a quick trial.
  network           = "default"
  control_plane_url = "https://control.example.com"
  release_source    = "gs://my-release-bucket"
  acme_email        = "ops@example.com"
}

output "address" {
  value = module.gateway.address
}

output "registration_token_secret" {
  value = module.gateway.registration_token_secret
}
