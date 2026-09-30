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

variable "mode" {
  description = "`single-tenant` (one org; GitHub OIDC audience is this gateway) or `platform` (SkiMasque's shared multi-tenant gateway; needs a skimasque-server release that supports --platform)."
  type        = string
  default     = "single-tenant"

  validation {
    condition     = contains(["single-tenant", "platform"], var.mode)
    error_message = "mode must be \"single-tenant\" or \"platform\"."
  }
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
    condition     = startswith(var.release_source, "gs://")
    error_message = "release_source must be gs://BUCKET."
  }
}

variable "acme_email" {
  description = "Contact email for the Let's Encrypt account."
  type        = string
}

variable "extra_args" {
  description = "Extra skimasque-server arguments, appended to the generated command line."
  type        = string
  default     = ""
}

variable "labels" {
  description = "Labels applied to the VM and disk."
  type        = map(string)
  default     = {}
}
