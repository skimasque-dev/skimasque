{{- define "skimasque-gateway.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "skimasque-gateway.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name (include "skimasque-gateway.name" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "skimasque-gateway.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
app.kubernetes.io/name: {{ include "skimasque-gateway.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "skimasque-gateway.selectorLabels" -}}
app.kubernetes.io/name: {{ include "skimasque-gateway.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "skimasque-gateway.image" -}}
{{- printf "%s:%s" .Values.image.repository (default .Chart.AppVersion .Values.image.tag) -}}
{{- end -}}

{{/*
The full argument list for skimasque-server, as a YAML sequence.
*/}}
{{- define "skimasque-gateway.args" -}}
{{- $v := .Values -}}
- --listen
- 0.0.0.0:{{ $v.ports.quic }}
- --metrics-listen
- 0.0.0.0:{{ $v.ports.metrics }}
{{- if $v.authority }}
- --authority
- {{ $v.authority | quote }}
{{- end }}
{{- if $v.tls.secretName }}
- --cert
- /etc/skimasque/tls/tls.crt
- --key
- /etc/skimasque/tls/tls.key
{{- if $v.reload.tls }}
- --tls-reload
{{- end }}
{{- end }}
{{- if $v.policies }}
- --policy-dir
- /etc/skimasque/policies
{{- if $v.reload.policy }}
- --policy-reload
{{- end }}
{{- end }}
{{- if $v.connectTcp }}
- --connect-tcp
{{- end }}
{{- if $v.allowPrivate }}
- --allow-private
{{- end }}
{{- range $v.allowCidrs }}
- --allow-cidr
- {{ . | quote }}
{{- end }}
{{- range $v.allowPorts }}
- --allow-port
- {{ . | quote }}
{{- end }}
{{- if $v.audit.enabled }}
- --audit-log
- /var/lib/skimasque/audit.jsonl
{{- end }}
{{- if $v.auth.githubOidc.enabled }}
- --github-oidc
{{- range $v.auth.githubOidc.audiences }}
- --oidc-audience
- {{ . | quote }}
{{- end }}
{{- end }}
{{- with $v.limits.maxConnections }}
- --max-connections
- {{ . | quote }}
{{- end }}
{{- with $v.limits.maxConnectionRate }}
- --max-connection-rate
- {{ . | quote }}
{{- end }}
{{- with $v.limits.maxConnectionBurst }}
- --max-connection-burst
- {{ . | quote }}
{{- end }}
{{- with $v.limits.maxSourceConnectionRate }}
- --max-source-connection-rate
- {{ . | quote }}
{{- end }}
{{- with $v.limits.maxSourceConnectionBurst }}
- --max-source-connection-burst
- {{ . | quote }}
{{- end }}
{{- with $v.limits.maxExchangeRate }}
- --max-exchange-rate
- {{ . | quote }}
{{- end }}
{{- with $v.limits.maxExchangeBurst }}
- --max-exchange-burst
- {{ . | quote }}
{{- end }}
{{- with $v.limits.maxTunnelsPerConnection }}
- --max-tunnels-per-connection
- {{ . | quote }}
{{- end }}
{{- with $v.limits.tunnelIdleTimeout }}
- --tunnel-idle-timeout
- {{ . | quote }}
{{- end }}
{{- with $v.limits.maxConcurrentRequests }}
- --max-concurrent-requests
- {{ . | quote }}
{{- end }}
{{- with $v.limits.shutdownGrace }}
- --shutdown-grace
- {{ . | quote }}
{{- end }}
{{- if gt (int $v.verbosity) 0 }}
- -{{ repeat (int $v.verbosity) "v" }}
{{- end }}
{{- range $v.extraArgs }}
- {{ . | quote }}
{{- end }}
{{- end -}}
