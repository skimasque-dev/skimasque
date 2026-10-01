variable "name" {
  description = "Short name for this gateway, used in resource names (lowercase letters, digits, hyphens)."
  type        = string

  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{1,22}[a-z0-9]$", var.name))
    error_message = "name must be 3-24 characters: lowercase letters, digits and hyphens, starting with a letter."
  }
}

variable "hostname" {
  description = "The public DNS name this gateway is reached at (also its ACME certificate name). Point an A record at the `address` output."
  type        = string
}

variable "project_id" {
  description = "The GCP project."
  type        = string
}

variable "zone" {
  description = "The GCP zone, e.g. us-central1-a."
  type        = string

  validation {
    condition     = can(regex("^[a-z]+-[a-z0-9]+-[a-z]$", var.zone))
    error_message = "zone must look like us-central1-a."
  }
}

variable "network" {
  description = "The VPC network (name or self link) to attach to."
  type        = string
}

variable "subnetwork" {
  description = "The subnetwork (name or self link). Required for custom-mode VPCs."
  type        = string
  default     = null
}

variable "machine_type" {
  description = "The VM size."
  type        = string
  default     = "e2-small"
}

variable "image" {
  description = "The boot image. Must have cloud-init and Python 3 (Ubuntu LTS does)."
  type        = string
  default     = "ubuntu-os-cloud/ubuntu-2404-lts-amd64"
}

variable "state_disk_gb" {
  description = "Size of the persistent disk that holds the gateway's identity, audit log and ACME cache. It survives VM replacement."
  type        = number
  default     = 10
}

variable "control_plane_url" {
  description = "The control plane this gateway enrols with, e.g. https://control.example.com."
  type        = string

  validation {
    condition     = startswith(var.control_plane_url, "https://")
    error_message = "control_plane_url must be an https:// URL."
  }
}

variable "release_source" {
  description = "Where the deploy agent reads its channel pointer and binaries, as gs://BUCKET. The VM's service account is given no access to it here: grant `service_account_email` read access in your stack."
  type        = string

  validation {
    condition     = can(regex("^gs://[a-z0-9][a-z0-9._-]+$", var.release_source))
    error_message = "release_source must be gs://BUCKET (a bucket name, no path)."
  }
}

variable "acme_email" {
  description = "Contact email for the Let's Encrypt account."
  type        = string

  validation {
    condition     = can(regex("^[^\\r\\n]+$", var.acme_email))
    error_message = "acme_email must be a single line."
  }
}

variable "mode" {
  description = "`single-tenant`: one organisation, enrolled with a token minted for that org, GitHub OIDC audience https://<hostname>. `platform`: SkiMasque's shared multi-tenant gateway (skimasque-server --platform), enrolled with a platform registration token; each organisation's audience is https://<hostname>/o/<slug>. Needs skimasque-server 0.3.1 or newer."
  type        = string
  default     = "single-tenant"

  validation {
    condition     = contains(["single-tenant", "platform"], var.mode)
    error_message = "mode must be single-tenant or platform."
  }
}

variable "extra_args" {
  description = "Extra skimasque-server arguments, appended to the generated command line. Never put a secret here: it lands in instance metadata and Terraform state."
  type        = string
  default     = ""

  validation {
    condition     = can(regex("^[^\\r\\n]*$", var.extra_args))
    error_message = "extra_args must be a single line."
  }
}

variable "labels" {
  description = "Labels applied to the VM and disk."
  type        = map(string)
  default     = {}
}
