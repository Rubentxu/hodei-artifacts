# Un solo parser HRN

Desde HSP-33, `kernel::Hrn::from_string` delega en `hodei-hrn 0.2.1`. Esto elimina la divergencia que permitía al borde HTTP aceptar paths con segmentos vacíos o formas que la autorización rechazaba después.

## Acciones

Las acciones son verbos, no recursos: el contrato canónico rechaza `...:Action/read` como HRN de recurso. Para los puntos que aún modelan una acción como `Hrn` existe `Hrn::from_action_string`, que exige `resource_type = Action`, un identificador no vacío y los seis campos del HRN. No relaja el parser de recursos.

## Verificación

- Kernel: 236 tests unitarios + migración.
- Test `canonical_resource_parser_rejects_actions_and_lax_paths`: rechaza action HRN y `releases//v2`; acepta la acción solo por el constructor explícito.
- `hrn_parity` de Artifacts compara el kernel con `hodei-hrn 0.2.1`.
- HTTP: un HRN rechazado por el contrato devuelve 400 (HSP-30).

## Límite

La representación `Hrn` de acciones sigue siendo un puente del playground; la representación preferida es `ActionId`. Migrar todo el dominio a `ActionId` es una tarea separada si se elimina ese puente.
