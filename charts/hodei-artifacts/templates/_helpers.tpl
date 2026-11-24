{{- define "hodei.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "hodei.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "hodei.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
app.kubernetes.io/name: {{ include "hodei.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: hodei
{{- end -}}

{{- define "hodei.selectorLabels" -}}
app.kubernetes.io/name: {{ include "hodei.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: api
{{- end -}}

{{- define "hodei.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "hodei.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{/* Validates the profile once; every template calls it through the Deployment. */}}
{{- define "hodei.profile" -}}
{{- if not (has .Values.profile (list "standalone" "rocksdb-garage" "external")) -}}
{{- fail (printf "profile must be one of standalone, rocksdb-garage, external (got %q)" .Values.profile) -}}
{{- end -}}
{{- .Values.profile -}}
{{- end -}}

{{/* "true" when the profile keeps metadata in embedded RocksDB. */}}
{{- define "hodei.usesRocksdb" -}}
{{- if ne (include "hodei.profile" .) "external" -}}true{{- end -}}
{{- end -}}

{{/* RocksDB admits a single writer: force 1 replica; external honours replicaCount. */}}
{{- define "hodei.replicas" -}}
{{- if include "hodei.usesRocksdb" . -}}1{{- else -}}
{{- if lt (int .Values.replicaCount) 2 -}}
{{- fail "profile external requires replicaCount >= 2" -}}
{{- end -}}
{{- .Values.replicaCount -}}
{{- end -}}
{{- end -}}

{{- define "hodei.image" -}}
{{- if .Values.image.digest -}}
{{- printf "%s@%s" .Values.image.repository .Values.image.digest -}}
{{- else -}}
{{- printf "%s:%s" .Values.image.repository (default .Chart.AppVersion .Values.image.tag) -}}
{{- end -}}
{{- end -}}

{{- define "hodei.garage.fullname" -}}
{{- printf "%s-garage" (include "hodei.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "hodei.dgraph.fullname" -}}
{{- printf "%s-dgraph" (include "hodei.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/* S3 endpoint host:port: explicit value, else in-cluster Garage. */}}
{{- define "hodei.s3.endpoint" -}}
{{- if .Values.s3.endpoint -}}
{{- .Values.s3.endpoint -}}
{{- else if .Values.garage.enabled -}}
{{- printf "%s.%s.svc:3900" (include "hodei.garage.fullname" .) .Release.Namespace -}}
{{- end -}}
{{- end -}}

{{/* S3 region: Garage always answers as "garage" unless s3.region is set. */}}
{{- define "hodei.s3.region" -}}
{{- if .Values.s3.region -}}{{- .Values.s3.region -}}
{{- else if .Values.garage.enabled -}}garage{{- end -}}
{{- end -}}

{{/* Converts 10G / 500M / 2T / 1024 into a byte count (decimal units, as Garage). */}}
{{- define "hodei.bytes" -}}
{{- $s := toString . -}}
{{- if regexMatch "^[0-9]+$" $s -}}{{ $s }}
{{- else if regexMatch "^[0-9]+[KMGT]$" $s -}}
{{- $n := int64 (regexFind "^[0-9]+" $s) -}}
{{- $u := regexFind "[KMGT]$" $s -}}
{{- $mul := dict "K" 1000 "M" 1000000 "G" 1000000000 "T" 1000000000000 -}}
{{- mul $n (int64 (get $mul $u)) -}}
{{- else -}}
{{- fail (printf "invalid capacity %q (use bytes or a K, M, G, T suffix)" $s) -}}
{{- end -}}
{{- end -}}
