output "name" {
  description = "The VM's name."
  value       = google_compute_instance.gateway.name
}

output "region" {
  description = "The GCP region the gateway runs in."
  value       = local.region
}

output "address" {
  description = "The gateway's static public IP. Create an A record for `hostname` pointing at it."
  value       = google_compute_address.gateway.address
}

output "service_account_email" {
  description = "The VM's identity. Grant it read access to the release bucket (objectViewer on the `gateway/` and `channels/` prefixes)."
  value       = google_service_account.gateway.email
}

output "registration_token_secret" {
  description = "The Secret Manager secret to put the one-time registration token in (`gcloud secrets versions add`)."
  value       = google_secret_manager_secret.registration_token.secret_id
}

output "rendered_cloud_init" {
  description = "The cloud-init user-data given to the VM, for inspection and tests. Contains no secrets."
  value       = local.cloud_init
}
