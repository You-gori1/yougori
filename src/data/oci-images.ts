export interface OciImageOption {
  value: string
  label: string
  description: string
  category: "Operating systems" | "Languages" | "Web servers" | "Data services" | "Developer tools"
}

export interface OciImageGroup {
  value: OciImageOption["category"]
  items: OciImageOption[]
}

export const customOciImage: OciImageOption = {
  value: "__custom__",
  label: "Custom image",
  description: "Enter any OCI-compatible registry reference",
  category: "Developer tools",
}

export const ociImages: OciImageOption[] = [
  { category: "Operating systems", label: "Alpine 3.24 (Yougori default)", value: "docker.io/library/alpine:3.24", description: "Small, maintained Linux base for current development tools" },
  { category: "Operating systems", label: "Alpine legacy (Quay)", value: "quay.io/libpod/alpine:latest", description: "Legacy Alpine 3.10 image; too old for many current coding tools. Prefer Alpine 3.24." },
  { category: "Operating systems", label: "Alpine", value: "docker.io/library/alpine:latest", description: "Minimal Linux base" },
  { category: "Operating systems", label: "Ubuntu", value: "docker.io/library/ubuntu:latest", description: "Popular Debian-based Linux" },
  { category: "Operating systems", label: "Debian", value: "docker.io/library/debian:latest", description: "Stable general-purpose Linux" },
  { category: "Operating systems", label: "Fedora", value: "quay.io/fedora/fedora:latest", description: "Current Fedora Linux base" },
  { category: "Operating systems", label: "Rocky Linux", value: "docker.io/rockylinux/rockylinux:latest", description: "Enterprise Linux compatible" },
  { category: "Operating systems", label: "AlmaLinux", value: "quay.io/almalinuxorg/almalinux:latest", description: "Enterprise Linux compatible" },
  { category: "Operating systems", label: "Oracle Linux", value: "docker.io/library/oraclelinux:latest", description: "Oracle enterprise Linux" },
  { category: "Operating systems", label: "Amazon Linux", value: "public.ecr.aws/amazonlinux/amazonlinux:latest", description: "AWS-optimized Linux base" },
  { category: "Operating systems", label: "Arch Linux", value: "docker.io/library/archlinux:latest", description: "Rolling-release Linux" },
  { category: "Operating systems", label: "openSUSE Leap", value: "docker.io/opensuse/leap:latest", description: "Stable openSUSE release" },
  { category: "Operating systems", label: "openSUSE Tumbleweed", value: "docker.io/opensuse/tumbleweed:latest", description: "Rolling openSUSE release" },
  { category: "Operating systems", label: "BusyBox", value: "docker.io/library/busybox:latest", description: "Extremely small Unix toolkit" },
  { category: "Operating systems", label: "Kali Linux", value: "docker.io/kalilinux/kali-rolling:latest", description: "Security testing distribution" },
  { category: "Operating systems", label: "Clear Linux", value: "docker.io/library/clearlinux:latest", description: "Performance-focused Linux" },
  { category: "Operating systems", label: "CentOS Stream 9", value: "quay.io/centos/centos:stream9", description: "Continuously delivered enterprise Linux" },
  { category: "Operating systems", label: "Red Hat UBI 9", value: "registry.access.redhat.com/ubi9/ubi:latest", description: "Freely redistributable Universal Base Image" },
  { category: "Operating systems", label: "Red Hat UBI 9 Minimal", value: "registry.access.redhat.com/ubi9/ubi-minimal:latest", description: "Compact Universal Base Image" },

  { category: "Languages", label: "Node.js", value: "docker.io/library/node:alpine", description: "Node.js on Alpine" },
  { category: "Languages", label: "Node.js Slim", value: "docker.io/library/node:slim", description: "Node.js on Debian Slim" },
  { category: "Languages", label: "Python", value: "docker.io/library/python:alpine", description: "Python on Alpine" },
  { category: "Languages", label: "Python Slim", value: "docker.io/library/python:slim", description: "Python on Debian Slim" },
  { category: "Languages", label: "Go", value: "docker.io/library/golang:alpine", description: "Go toolchain on Alpine" },
  { category: "Languages", label: "Rust", value: "docker.io/library/rust:slim", description: "Rust toolchain on Debian Slim" },
  { category: "Languages", label: "Ruby", value: "docker.io/library/ruby:alpine", description: "Ruby on Alpine" },
  { category: "Languages", label: "PHP CLI", value: "docker.io/library/php:cli-alpine", description: "PHP command-line runtime" },
  { category: "Languages", label: "Perl", value: "docker.io/library/perl:slim", description: "Perl on Debian Slim" },
  { category: "Languages", label: "Java", value: "docker.io/library/eclipse-temurin:latest", description: "Eclipse Temurin OpenJDK" },
  { category: "Languages", label: "Maven", value: "docker.io/library/maven:latest", description: "Java and Maven build tools" },
  { category: "Languages", label: "Gradle", value: "docker.io/library/gradle:latest", description: "Java and Gradle build tools" },
  { category: "Languages", label: "Swift", value: "docker.io/library/swift:latest", description: "Swift development environment" },
  { category: "Languages", label: "Dart", value: "docker.io/library/dart:stable", description: "Dart SDK" },
  { category: "Languages", label: "GCC", value: "docker.io/library/gcc:latest", description: "GNU compiler toolchain" },
  { category: "Languages", label: ".NET SDK", value: "mcr.microsoft.com/dotnet/sdk:9.0", description: "Microsoft .NET development kit" },
  { category: "Languages", label: ".NET Runtime", value: "mcr.microsoft.com/dotnet/runtime:9.0", description: "Microsoft .NET application runtime" },
  { category: "Languages", label: "Deno", value: "ghcr.io/denoland/deno:alpine", description: "JavaScript and TypeScript runtime" },
  { category: "Languages", label: "Python with uv", value: "ghcr.io/astral-sh/uv:python3.13-alpine", description: "Python with the uv package manager" },

  { category: "Web servers", label: "Nginx", value: "docker.io/library/nginx:alpine", description: "High-performance web server" },
  { category: "Web servers", label: "Apache HTTP Server", value: "docker.io/library/httpd:alpine", description: "Apache web server" },
  { category: "Web servers", label: "Caddy", value: "docker.io/library/caddy:alpine", description: "Automatic HTTPS web server" },
  { category: "Web servers", label: "HAProxy", value: "docker.io/library/haproxy:alpine", description: "Load balancer and proxy" },
  { category: "Web servers", label: "Traefik", value: "docker.io/library/traefik:latest", description: "Cloud-native application proxy" },
  { category: "Web servers", label: "Tomcat", value: "docker.io/library/tomcat:latest", description: "Java servlet container" },
  { category: "Web servers", label: "Keycloak", value: "quay.io/keycloak/keycloak:latest", description: "Identity and access management" },
  { category: "Web servers", label: "Prometheus", value: "quay.io/prometheus/prometheus:latest", description: "Metrics monitoring server" },

  { category: "Data services", label: "PostgreSQL", value: "docker.io/library/postgres:latest", description: "Relational database" },
  { category: "Data services", label: "MySQL", value: "docker.io/library/mysql:latest", description: "Relational database" },
  { category: "Data services", label: "MariaDB", value: "docker.io/library/mariadb:latest", description: "MySQL-compatible database" },
  { category: "Data services", label: "MongoDB", value: "docker.io/library/mongo:latest", description: "Document database" },
  { category: "Data services", label: "Redis", value: "docker.io/library/redis:alpine", description: "In-memory data store" },
  { category: "Data services", label: "Memcached", value: "docker.io/library/memcached:alpine", description: "Distributed memory cache" },
  { category: "Data services", label: "RabbitMQ", value: "docker.io/library/rabbitmq:alpine", description: "Message broker" },
  { category: "Data services", label: "Eclipse Mosquitto", value: "docker.io/library/eclipse-mosquitto:latest", description: "MQTT message broker" },
  { category: "Data services", label: "CouchDB", value: "docker.io/library/couchdb:latest", description: "JSON document database" },
  { category: "Data services", label: "Neo4j", value: "docker.io/library/neo4j:latest", description: "Graph database" },
  { category: "Data services", label: "MinIO", value: "quay.io/minio/minio:latest", description: "S3-compatible object storage" },

  { category: "Developer tools", label: "Git", value: "docker.io/alpine/git:latest", description: "Small Git environment" },
  { category: "Developer tools", label: "curl", value: "docker.io/curlimages/curl:latest", description: "HTTP command-line client" },
  { category: "Developer tools", label: "Composer", value: "docker.io/library/composer:latest", description: "PHP dependency manager" },
  { category: "Developer tools", label: "Docker Registry", value: "docker.io/library/registry:latest", description: "OCI image registry server" },
  { category: "Developer tools", label: "Adminer", value: "docker.io/library/adminer:latest", description: "Database administration UI" },
  { category: "Developer tools", label: "phpMyAdmin", value: "docker.io/library/phpmyadmin:latest", description: "MySQL administration UI" },
  { category: "Developer tools", label: "Podman", value: "quay.io/podman/stable:latest", description: "Daemonless OCI container tools" },
  { category: "Developer tools", label: "Buildah", value: "quay.io/buildah/stable:latest", description: "OCI image build tools" },
  { category: "Developer tools", label: "Skopeo", value: "quay.io/skopeo/stable:latest", description: "Remote image inspection tools" },
  { category: "Developer tools", label: "Microsoft Dev Container", value: "mcr.microsoft.com/devcontainers/base:ubuntu", description: "Ubuntu development environment" },
  { category: "Developer tools", label: "Azure CLI", value: "mcr.microsoft.com/azure-cli:latest", description: "Microsoft Azure command-line tools" },
  { category: "Developer tools", label: "AWS SAM Python Builder", value: "public.ecr.aws/sam/build-python3.13:latest", description: "AWS serverless Python build environment" },
  customOciImage,
]

const categoryOrder: OciImageOption["category"][] = ["Operating systems", "Languages", "Web servers", "Data services", "Developer tools"]

export const ociImageGroups: OciImageGroup[] = categoryOrder.map((value) => ({
  value,
  items: ociImages.filter((image) => image.category === value),
}))

export const defaultOciImage = ociImages[0]!

export function ociStartupCommand(image: OciImageOption): string {
  return image.category === "Operating systems" || image.category === "Languages"
    ? "sleep 2147483647"
    : ""
}

export function ociRegistryLabel(reference: string): string {
  const host = reference.split("/", 1)[0] ?? reference
  return ({
    "docker.io": "Docker Hub",
    "ghcr.io": "GitHub",
    "mcr.microsoft.com": "Microsoft",
    "public.ecr.aws": "Amazon ECR",
    "quay.io": "Quay",
    "registry.access.redhat.com": "Red Hat",
  } as Record<string, string>)[host] ?? host
}
