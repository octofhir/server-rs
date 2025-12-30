# OctoFHIR App IG

Application-level resources for OctoFHIR configuration and management.

## Resources

- **App** - Application configuration and metadata
- **CustomOperation** - Custom FHIR operations
- **IdentityProvider** - External IdP federation configuration

## Routes

All resources in this IG are accessible WITHOUT the `/fhir` prefix:

- `GET /App` - List apps
- `GET /CustomOperation` - List custom operations
- `GET /IdentityProvider` - List identity providers

## Purpose

This IG contains resources for configuring OctoFHIR itself, separate from auth (users/sessions) and FHIR data (patients/observations).
