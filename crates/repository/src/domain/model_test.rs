#[cfg(test)]
mod tests {
    use crate::domain::model::{Repository, RepositoryName, RepositoryDescription};
    use shared::{RepositoryId, UserId, IsoTimestamp};
    use uuid::Uuid;

    /// Verifica que Repository::new inicializa correctamente los campos y asigna created_at (no futuro).
    #[test]
    fn repository_new_initializes_fields() {
        let id = RepositoryId(Uuid::new_v4());
        let user = UserId(Uuid::new_v4());
        let name = RepositoryName::new("repo-alpha");
        let desc = Some(RepositoryDescription("Descripción inicial".to_string()));

        let before = IsoTimestamp::now();
        let repo = Repository::new(id.clone(), name.clone(), desc.clone(), user.clone());
        let after = IsoTimestamp::now();

        // Id y campos básicos
        assert_eq!(repo.id.0, id.0, "ID debe preservarse");
        assert_eq!(repo.name.0, name.0, "Nombre debe preservarse");
        assert_eq!(repo.description.as_ref().unwrap().0, desc.unwrap().0, "Descripción debe preservarse");
        assert_eq!(repo.created_by.0, user.0, "created_by debe preservarse");

        // created_at dentro del intervalo [before, after]
        assert!(repo.created_at.0 >= before.0, "created_at debe ser >= timestamp before");
        assert!(repo.created_at.0 <= after.0, "created_at debe ser <= timestamp after");
    }

    /// Verifica que RepositoryName::new acepta distintos tipos Into<String> (ergonomía).
    #[test]
    fn repository_name_new_accepts_into_string() {
        let raw = "repo-x";
        let n1 = RepositoryName::new(raw);
        let n2 = RepositoryName::new(raw.to_string());
        assert_eq!(n1.0, raw);
        assert_eq!(n2.0, raw);
    }

    /// Verifica que Repository puede crearse sin descripción.
    #[test]
    fn repository_new_without_description() {
        let repo = Repository::new(
            RepositoryId(Uuid::new_v4()),
            RepositoryName::new("repo-no-desc"),
            None,
            UserId(Uuid::new_v4()),
        );
        assert!(repo.description.is_none(), "Descripción debe ser None");
    }

    /// (Placeholder) Test futuro para validación de nombre (REPO-T3).
    #[test]
    fn repository_name_validation_placeholder() {
        let _name = RepositoryName::new("repo_PlaceHolder-123");
        // Cuando REPO-T3 se implemente, aquí se añadirán asserts de validación.
    }
}
