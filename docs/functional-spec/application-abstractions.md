---
title: Application-provided abstractions
status: draft
last-reviewed: 2026-09-24
related:
  - scope.md
  - functional-requirements.md
  - open-questions.md
---

# Application-provided abstractions

## Overview

The crate defines each abstraction below as a Rust trait and calls it. The application implements it and honours the contract the crate defines. This list is closed: an abstraction not listed here requires a change to this specification. The crate has no clock abstraction ([FR-TYPE-006](functional-requirements.md#types)) and no time-zone data abstraction ([FR-TYPE-007](functional-requirements.md#types)).

## Abstractions

- ABS-001 **Transport.** The application SHALL supply, through this abstraction, the connections that the crate's Bolt endpoints use: incoming connections for the server-side endpoint and outgoing connections for the client-side endpoint. Sockets, TLS and accepting connections remain with the application.
- ABS-002 **Authentication and authorisation.** The crate SHALL pass the authentication scheme and credentials to this abstraction, which SHALL return either a principal or a refusal; for a Bolt session it MAY also return the session's home schema and home graph ([FR-SES-002](functional-requirements.md#sessions)). The crate SHALL also delegate `imp_user` (impersonation) requests to it ([FR-BOLT-012](functional-requirements.md#bolt)). Before each catalog action and each data action, the crate SHALL ask it whether the principal may perform the action on the object, passing the object's owner ([FR-AUTH-001](functional-requirements.md#authorisation), [FR-AUTH-002](functional-requirements.md#authorisation)). Rust API sessions receive their principal from the application and are not authenticated ([FR-SES-001](functional-requirements.md#sessions)).
- ABS-003 **Storage.** The crate SHALL perform storage I/O through this abstraction. The crate SHALL provide a default filesystem implementation; the application MAY replace it.
- ABS-004 **Metrics sink.** The crate SHALL deliver its metrics to this abstraction, which the application connects ([FR-OBS-001](functional-requirements.md#observability)).
- ABS-005 **Procedures and functions.** The application SHALL implement its registered procedures and functions through this abstraction, and the crate SHALL call them when a GQL request invokes them ([FR-API-005](functional-requirements.md#api), [FR-PROC-001](functional-requirements.md#procedures-and-functions) to [FR-PROC-003](functional-requirements.md#procedures-and-functions)).
