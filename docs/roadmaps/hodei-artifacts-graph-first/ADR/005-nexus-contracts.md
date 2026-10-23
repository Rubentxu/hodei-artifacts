# ADR-005 — Nexus CE como oráculo de protocolos, no arquitectura fuente
**Estado:** ACEPTADA COMO DIRECCIÓN.
**Decisión:** formalizar contratos observables de versiones pinneadas Nexus CE y especificaciones oficiales Maven/OCI/npm/PyPI; cada caso VERIFIED usa cliente real y evidencia; mantener diferencias intencionales de seguridad documentadas. CogniCode analiza código Java público; REA estudia binarios/red con permiso sin inferir fuentes que no existen.
**Alternativas:** transliterar repositorio público Java a Rust (rechazado por deuda y licencias), prometer paridad CE por documentar (rechazado).
**Gates:** matriz de contratos y 1–3 tests de aceptación efímeros por wave; no declarar paridad sobre formatos sin verificar.
