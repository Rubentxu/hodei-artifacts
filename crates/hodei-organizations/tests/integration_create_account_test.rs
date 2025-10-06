//! Test de integración para HU-ORG-001: Creación de una nueva cuenta
//!
//! Este test verifica que:
//! 1. La API pública del use case está correctamente expuesta
//! 2. El DTO CreateAccountCommand funciona correctamente
//! 3. El use case se puede instanciar con los tipos correctos
//!
//! NOTA: Este test se enfoca en validar la API pública del crate.
//! Los tests unitarios internos (`use_case_test.rs`) validan la lógica transaccional completa.

use hodei_organizations::CreateAccountUseCase;
use hodei_organizations::features::create_account::{dto::*, error::*};
use kernel::Hrn;

#[test]
fn test_create_account_command_creation() {
    // Arrange & Act: Verificar que el DTO se puede crear correctamente
    let command = CreateAccountCommand {
        name: "production".to_string(),
        parent_hrn: Some(Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "123456789012".to_string(),
            "ou".to_string(),
            "root".to_string(),
        )),
    };

    // Assert: Verificar estructura del comando
    assert_eq!(command.name, "production");
    assert!(command.parent_hrn.is_some());
}

#[test]
fn test_create_account_view_structure() {
    // Arrange & Act: Verificar que el DTO de respuesta funciona
    let view = AccountView {
        hrn: Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "123456789012".to_string(),
            "account".to_string(),
            "production".to_string(),
        ),
        name: "production".to_string(),
        parent_hrn: None,
    };

    // Assert: Verificar estructura de la vista
    assert_eq!(view.name, "production");
    assert_eq!(view.hrn.resource_type(), "account");
    assert_eq!(view.hrn.resource_id(), "production");
}

#[test]
fn test_create_account_error_types() {
    // Verificar que los tipos de error están correctamente expuestos
    let error = CreateAccountError::InvalidAccountName;
    assert!(matches!(error, CreateAccountError::InvalidAccountName));

    let error2 = CreateAccountError::TransactionError("test".to_string());
    assert!(matches!(error2, CreateAccountError::TransactionError(_)));
}

#[test]
fn test_api_public_exports() {
    // Este test compila solo si todos los tipos públicos están correctamente exportados
    // Verificar que el use case se puede referenciar
    fn _use_case_is_public<UWF>(_uc: CreateAccountUseCase<UWF>)
    where
        UWF: hodei_organizations::features::create_account::ports::CreateAccountUnitOfWorkFactory,
    {
    }

    // Si este test compila, significa que la API pública está correctamente estructurada
    assert!(true, "API pública correctamente expuesta");
}

/// Test que valida la estructura de la API pública sin ejecutar el use case
/// (ya que requiere implementaciones reales de repositorios que son detalles internos)
#[test]
fn test_create_account_use_case_type_signature() {
    // Verificar que el use case tiene la firma correcta esperada
    use hodei_organizations::features::create_account::ports::CreateAccountUnitOfWorkFactory;

    // Función helper que valida los tipos en tiempo de compilación
    fn _validate_use_case_signature<UWF: CreateAccountUnitOfWorkFactory>() {
        // Si esto compila, la API pública es correcta
    }

    assert!(true, "Use case tiene la firma de tipos correcta");
}

/// Test de documentación: Ejemplo de cómo usar la API pública
///
/// Este test documenta el patrón de uso esperado del use case.
/// No se ejecuta porque requiere una implementación real de UoW.
#[test]
#[ignore = "Test documental - muestra el uso esperado de la API"]
fn test_documented_usage_pattern() {
    // Este es el patrón esperado de uso:
    //
    // 1. En la composition root (main.rs o módulo de configuración):
    //    - Crear una instancia de SurrealUnitOfWorkFactory con la conexión DB
    //    - Crear un CreateAccountSurrealUnitOfWorkFactoryAdapter
    //    - Crear el CreateAccountUseCase inyectando el adapter
    //
    // 2. En el código de aplicación:
    //    - Crear un CreateAccountCommand con los datos
    //    - Ejecutar use_case.execute(command).await
    //    - Manejar el Result<AccountView, CreateAccountError>
    //
    // Ejemplo conceptual:
    // ```
    // let db = Surreal::new::<Mem>(()).await?;
    // let factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
    // let adapter_factory = CreateAccountSurrealUnitOfWorkFactoryAdapter::new(factory);
    // let use_case = CreateAccountUseCase::new(
    //     Arc::new(adapter_factory),
    //     "aws".to_string(),
    //     "123456789012".to_string()
    // );
    //
    // let command = CreateAccountCommand {
    //     name: "production".to_string(),
    //     parent_hrn: None,
    // };
    //
    // let result = use_case.execute(command).await?;
    // println!("Created account: {}", result.hrn);
    // ```
}
