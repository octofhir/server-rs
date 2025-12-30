# OctoFHIR Auth IG

Authentication and authorization resources for OctoFHIR.

## Resources

- **User** - User accounts with credentials and profile information
- **Session** - Active user sessions
- **Client** - OAuth 2.0 / SMART on FHIR clients
- **AccessPolicy** - Fine-grained access control policies
- **Role** - User roles for RBAC
- **RefreshToken** - OAuth 2.0 refresh tokens
- **RevokedToken** - Revoked tokens blacklist

## Routes

All resources in this IG are accessible WITHOUT the `/fhir` prefix:

- `GET /User` - List users
- `POST /User` - Create user
- `GET /User/:id` - Read user
- `PUT /User/:id` - Update user
- `DELETE /User/:id` - Delete user

Similar pattern for Session, Client, AccessPolicy, Role.

## Security

- All routes require AdminAuth middleware
- Credentials (passwords, tokens) are encrypted at rest
- Password hashes use Argon2
- Tokens are JWT with RS256/RS384/ES384
