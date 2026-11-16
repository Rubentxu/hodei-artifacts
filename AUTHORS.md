# AUTHORS

Los créditos de este repositorio van aquí, no en el mensaje de los commits.

La razón es una regla de la máquina que lo gobierna: el hook de
`reference-transaction` rechaza cualquier commit cuyo mensaje lleve un
trailer `Co-authored-by`, y un *squash* de GitHub los añade solo cuando la
rama tiene más de una firma. Un commit así no puede entrar en `main`: la
referencia se queda sin poder avanzar y el clon deja de sincronizar.

Este fichero es el sitio donde esa información vive, y donde la política dice
que debe vivir.

## Identidades

Cada commit lo firma una de estas identidades. La política de la máquina rota
la firma entre el pool de perfiles que esta tabla enumera, así que una
persona aparece en la historia con más de un nombre.

| Identidad | Correo |
| --- | --- |
| Ruben Dario Cabrera Garcia | `rubentxu74@gmail.com` |
| Rubentxu | `rubentxu@users.noreply.github.com` |
| RubentxuDev | `rubentxudev@gmail.com` |
| Pablo Cabrera | `pabloformacion13@gmail.com` |
| Nomeacuerdo | `nomeacuerdodelputocorreo@gmail.com` |
| Ilargia | `ilargia.c.g@gmail.com` |
| Haizea | `haizea.cabrera.g@gmail.com` |
| SocketStudio | `socketstuido@gmail.com` |

## How to credit a merged pull request

Cuando un *squash* collapse el trabajo de más de una firma, sus autores no
quedan en la historia de `main`: quedan en la rama y en la pull request. Para
que no se pierdan, el mensaje del *squash* va sin trailers y la atribución se
declara aquí, en la fila de la entrega.

Cuando el hosting genera el mensaje solo, el mensaje se reescribe antes de
que entre en la rama: el hook juzga el commit otra vez antes de dejar que la
referencia avance, y un mensaje con trailers no pasa.
