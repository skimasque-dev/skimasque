# The SkiMasque deploy agent's files, as module outputs.
#
# One source of truth: the gateway module and the control-plane stack both
# embed these into cloud-init, so a VM can never run a different copy of the
# agent than the one in this repository.

terraform {
  required_version = ">= 1.6"
}

output "deploy_script" {
  description = "deploy/agent/skimasque_deploy.py, installed as /usr/local/sbin/skimasque-deploy."
  value       = file("${path.module}/../../agent/skimasque_deploy.py")
}

output "deploy_service" {
  description = "The systemd unit that runs the agent once."
  value       = file("${path.module}/../../agent/skimasque-deploy.service")
}

output "deploy_timer" {
  description = "The systemd timer that runs the agent every two minutes."
  value       = file("${path.module}/../../agent/skimasque-deploy.timer")
}

output "prepare_disk_script" {
  description = "deploy/agent/skimasque-prepare-disk, installed as /usr/local/sbin/skimasque-prepare-disk."
  value       = file("${path.module}/../../agent/skimasque-prepare-disk")
}
