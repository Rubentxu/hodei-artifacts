//! Adaptadores de infraestructura para el bounded context Analytics.
//! Aquí vivirán implementaciones concretas de los puertos definidos en application::ports
//! (p.ej. repositorios MongoDB, publicadores Kafka, endpoints HTTP Axum, etc.)

// Mantener este módulo sin dependencias cíclicas hacia features. Los features dependen de puertos,
// y la infraestructura implementa esos puertos.

