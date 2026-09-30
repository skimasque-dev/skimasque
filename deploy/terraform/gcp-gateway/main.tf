module "agent_files" {
  source = "../agent-files"
}

locals {
  region    = join("-", slice(split("-", var.zone), 0, 2))
  secret_id = "skimasque-gw-${var.name}-registration-token"

  # The gateway's command line. Everything stable lives here so a change shows
  # up as a Terraform diff; the one-time registration token is NOT here -- it
  # arrives as SKIMASQUE_CONTROL_TOKEN from Secret Manager.
  server_args = join(" ", compact([
    "--listen 0.0.0.0:443",
    "--hostname ${var.hostname}",
    "--acme",
    "--acme-email ${var.acme_email}",
    "--acme-cache /var/lib/skimasque/acme",
    "--control-plane ${var.control_plane_url}",
    "--control-plane-state /var/lib/skimasque/control",
    "--control-plane-name ${var.hostname}",
    "--audit-log /var/lib/skimasque/audit.jsonl",
    "--metrics-listen 127.0.0.1:9090",
    "--github-oidc",
    "--oidc-audience https://${var.hostname}",
    var.extra_args,
  ]))

  cloud_init = templatefile("${path.module}/cloud-init/gateway.yaml.tftpl", {
    deploy_script_b64       = base64encode(module.agent_files.deploy_script)
    deploy_service_b64      = base64encode(module.agent_files.deploy_service)
    deploy_timer_b64        = base64encode(module.agent_files.deploy_timer)
    prepare_disk_script_b64 = base64encode(module.agent_files.prepare_disk_script)
    gateway_unit_b64        = base64encode(file("${path.module}/../../systemd/skimasque-gateway.service"))
    server_args             = local.server_args
    release_source          = var.release_source
    project_id              = var.project_id
    secret_id               = local.secret_id
  })
}

resource "google_service_account" "gateway" {
  project      = var.project_id
  account_id   = substr("gw-${var.name}", 0, 30)
  display_name = "SkiMasque gateway ${var.name}"
}

resource "google_compute_address" "gateway" {
  project = var.project_id
  name    = "skimasque-gw-${var.name}"
  region  = local.region
}

# The gateway's identity (granted by a one-time token), audit log and ACME
# account live here, so replacing the VM does not orphan the gateway.
resource "google_compute_disk" "state" {
  project = var.project_id
  name    = "skimasque-gw-${var.name}-state"
  zone    = var.zone
  type    = "pd-balanced"
  size    = var.state_disk_gb
  labels  = var.labels

  lifecycle {
    prevent_destroy = true
  }
}

resource "google_compute_firewall" "gateway" {
  project       = var.project_id
  name          = "skimasque-gw-${var.name}-ingress"
  network       = var.network
  direction     = "INGRESS"
  source_ranges = ["0.0.0.0/0"]
  target_tags   = ["skimasque-gw-${var.name}"]

  allow {
    protocol = "tcp"
    ports    = ["443"]
  }

  allow {
    protocol = "udp"
    ports    = ["443"]
  }
}

# SSH only through Identity-Aware Proxy (break-glass; deploys never need it).
resource "google_compute_firewall" "iap_ssh" {
  project       = var.project_id
  name          = "skimasque-gw-${var.name}-iap-ssh"
  network       = var.network
  direction     = "INGRESS"
  source_ranges = ["35.235.240.0/20"]
  target_tags   = ["skimasque-gw-${var.name}"]

  allow {
    protocol = "tcp"
    ports    = ["22"]
  }
}

# The one-time registration token is added by an operator, never by Terraform.
resource "google_secret_manager_secret" "registration_token" {
  project   = var.project_id
  secret_id = local.secret_id

  replication {
    auto {}
  }
}

resource "google_secret_manager_secret_iam_member" "registration_token" {
  project   = var.project_id
  secret_id = google_secret_manager_secret.registration_token.secret_id
  role      = "roles/secretmanager.secretAccessor"
  member    = "serviceAccount:${google_service_account.gateway.email}"
}

# cloud-init runs once per instance, so a change to it must replace the VM.
# That is safe here: state is on its own disk and the IP and DNS do not change.
resource "terraform_data" "cloud_init" {
  input = local.cloud_init
}

resource "google_compute_instance" "gateway" {
  project      = var.project_id
  name         = "skimasque-gw-${var.name}"
  zone         = var.zone
  machine_type = var.machine_type
  tags         = ["skimasque-gw-${var.name}"]
  labels       = var.labels

  allow_stopping_for_update = true

  boot_disk {
    initialize_params {
      image = var.image
      size  = 20
      type  = "pd-balanced"
    }
  }

  attached_disk {
    source      = google_compute_disk.state.self_link
    device_name = "skimasque-state"
    mode        = "READ_WRITE"
  }

  network_interface {
    network    = var.network
    subnetwork = var.subnetwork

    access_config {
      nat_ip = google_compute_address.gateway.address
    }
  }

  service_account {
    email  = google_service_account.gateway.email
    scopes = ["cloud-platform"]
  }

  shielded_instance_config {
    enable_secure_boot          = true
    enable_vtpm                 = true
    enable_integrity_monitoring = true
  }

  metadata = {
    user-data              = local.cloud_init
    enable-oslogin         = "TRUE"
    block-project-ssh-keys = "TRUE"
  }

  lifecycle {
    replace_triggered_by = [terraform_data.cloud_init]
  }
}
