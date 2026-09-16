# DevStory 4.5 - Como usuario freelance consultor quiero que exista un sistema de resolución de disputas en el que participe la DAO para evitar situaciones en las que el trabajo entregado se rechace continuamente

## Objetivo

Cada vez que se hace la entrega de un hito, este debe ser revisado y aprobado o rechazado por el cliente. La aprobación libera automáticamente los fondos asociados a la consecución de dicho hito, mientras que el rechazo constituye una consideración de que el entregable no cumple con los requisitos acordados y debe indicar los motivos del mismo. Este rechazo no implica que automáticamente se abra una disputa, ya que la disputa se considerará un mecanismo voluntario que permite escalar el conflico cuando el proceso ordinario de revisión y comunicación no permite resolverlo. Es conveniente aclarar que tanto el cliente como el coordinador del proyecto son responsables de la gestión del proyecto y podrán iniciar una disputa.

El proceso de apertura de disputa es un proceso muy importante para la plataforma, porque conlleva una toma de decisión que puede implicar en afectar a la credibilidad de la misma, por lo que los fundamentos de apertura de una disputa deben estar muy documentados, incluyendo todo el historial del proceso por el que se ha llegado a la disputa accesible para facilitar la toma de decisión: Definición del hito, criterios de aceptación, entregable, motivación de la disputa,... Todo ello es material importante que ayudará a la DAO a tener el contexto necesario para una futura resolución.

Se ha mencionado la DAO como entidad de gobernanza, que será la encargada de la resolución del conflicto. El mecanismo concreto de resolución y/o la asignación de una tercera parte imparcial (miembro de la DAO) quedan a la &nbsp;gobernanza de la DAO. Por lo que respecta a esta definición, se debe facilitar un mecanismo para que un &quot;juez&quot; emita resolución y los fundamentos de la resolución, el modelo de resolución y la asignación del juez se definirá en el modelo de gobernanza de la DAO.

## Descripción de la funcionalidad

Como usuario freelance consultor, quiero disponer de un mecanismo formal para abrir una disputa cuando considere que un hito está siendo rechazado de forma injustificada o que existe una controversia que no puede resolverse mediante el proceso habitual de comunicación, de forma que el conflicto quede registrado y pueda ser tratado posteriormente por la DAO.

La disputa podrá abrirse desde el **primer rechazo de un hito** y deberá estar siempre acompañada de una argumentación por parte de la entidad que la inicia.

Una vez abierta:

1.  Se crea un **registro de disputa** asociado al proyecto y al hito correspondiente.

2.  Se incorpora automáticamente el histórico disponible del hito/proyecto.

3.  La entidad que abre la disputa registra sus argumentos.

4.  La parte contra la que se abre la disputa recibe la notificación y puede incorporar sus propios argumentos.

5.  Se habilita automáticamente un **canal de comunicación** asociado a la disputa.

6.  La disputa pasa a un estado público, conforme a las reglas de visibilidad definidas por la plataforma.

7.  La disputa queda preparada para su posterior participación y resolución por la DAO.


## Flujo funcional

### 1\. Rechazo del hito

El cliente revisa un hito y decide rechazarlo.

El rechazo debe incluir obligatoriamente los motivos que justifican la decisión.

El sistema registra:

*   Hito rechazado.

*   Proyecto al que pertenece.

*   Cliente que realiza el rechazo.

*   Fecha y hora.

*   Motivos del rechazo.

*   Información del entregable y versión presentada.

*   Información disponible sobre las revisiones anteriores.


El rechazo habilita la posibilidad de abrir una disputa, pero **no la inicia automáticamente**.

### 2\. Apertura de la disputa

El cliente o el coordinador del proyecto pueden iniciar una disputa desde el primer rechazo del hito.

La apertura requiere:

*   Identificación del hito objeto de controversia.

*   Identificación de la entidad que inicia la disputa.

*   Argumentación obligatoria.

*   Registro de fecha y hora de apertura.


Al crear la disputa, el sistema incorpora automáticamente el histórico disponible del hito y del proyecto.

### 3\. Incorporación de argumentos

La entidad que inicia la disputa registra sus argumentos.

La parte contra la que se abre la disputa recibe acceso a la disputa y puede incorporar sus propios argumentos.

Las intervenciones deben conservar:

*   Autor.

*   Fecha y hora.

*   Tipo de intervención.

*   Contenido.

*   Relación con la disputa.


Las intervenciones no deben sobreescribirse, garantizando la trazabilidad completa de la evolución de la disputa.

### 4\. Canal de comunicación

La apertura de una disputa crea automáticamente un canal de comunicación asociado a ella.

Este canal permite a las partes continuar la comunicación sobre el conflicto manteniendo su relación directa con el expediente de la disputa.

### 5\. Publicación de la disputa

Una vez registrada la disputa y los argumentos iniciales, la disputa pasa a estado **pública**.

La publicación debe permitir consultar, como mínimo:

*   Identificación del proyecto.

*   Hito afectado.

*   Estado de la disputa.

*   Fecha de apertura.

*   Entidad que la ha iniciado.

*   Argumentación inicial.

*   Histórico relevante del hito.

*   Argumentos incorporados posteriormente.


La información sensible o privada que no forme parte del expediente público deberá mantenerse fuera de esta vista.

### 6\. Participación de la DAO

La disputa queda preparada para que la DAO pueda intervenir con:

*   Resolución de la disputa.

*   Votación de la DAO.

*   Selección de árbitros o tercera parte imparcial.

*   Ejecución de una resolución.

*   Modificación automática del escrow como consecuencia de una resolución.

*   Penalizaciones o compensaciones derivadas de la disputa.


## Requerimientos / Criterios de aceptación

### Criterio 1 - Rechazo con motivos

```gherkin
Given un cliente que está revisando un hito
When decide rechazar el hito
Then debe indicar los motivos del rechazo
And el sistema registra el rechazo asociado al hito
```

### Criterio 2 - Disputa desde el primer rechazo

```gherkin
Given un hito que ha sido rechazado por primera vez
When el cliente o el coordinador del proyecto consulta el hito
Then encuentra disponible la opción de abrir una disputa
```

### Criterio 3 - Disputa opcional

```gherkin
Given un hito rechazado
When ninguna de las partes quiere iniciar una disputa
Then el hito permanece en su flujo normal de revisión
And no se crea ninguna disputa automáticamente
```

### Criterio 4 - Apertura por cliente

```gherkin
Given un hito rechazado
When el cliente decide abrir una disputa
Then puede crear la disputa
And debe introducir obligatoriamente sus argumentos
```

### Criterio 5 - Apertura por coordinador

```gherkin
Given un hito rechazado
When el coordinador del proyecto decide abrir una disputa
Then puede crear la disputa
And debe introducir obligatoriamente sus argumentos
```

### Criterio 6 - Registro automático del contexto

```gherkin
Given una disputa que se está creando
When el usuario confirma su apertura
Then el sistema incorpora automáticamente el histórico disponible del hito y del proyecto
And la información queda asociada al expediente de la disputa
```

### Criterio 7 - Argumentos de la parte contraria

```gherkin
Given una disputa que ha sido abierta
When la parte contra la que se ha iniciado la disputa accede a ella
Then puede incorporar sus argumentos
And estos quedan registrados como parte del expediente de la disputa
```

### Criterio 8 - Trazabilidad de argumentos

```gherkin
Given una disputa con argumentos de ambas partes
When un usuario consulta su histórico
Then puede identificar quién realizó cada intervención
And puede consultar la fecha y hora de cada intervención
And las intervenciones originales no pueden ser sobrescritas
```

### Criterio 9 - Canal de comunicación

```gherkin
Given una disputa que acaba de ser abierta
When el sistema confirma la creación de la disputa
Then se crea automáticamente un canal de comunicación asociado a ella
And las partes pueden utilizarlo para comunicarse sobre el conflicto
```

### Criterio 10 - Publicación

```gherkin
Given una disputa correctamente creada
When la apertura ha sido confirmada
Then la disputa pasa a estado pública
And puede ser consultada conforme a las reglas de visibilidad definidas por la plataforma
```

### Criterio 11 - Estado de la disputa

```gherkin
Given una disputa recién creada
When el sistema registra su apertura
Then la disputa tiene un estado que identifica que está abierta y pendiente de resolución
```

### Criterio 12 - Preparación para la DAO

```gherkin
Given una disputa abierta y registrada
When la PoC finaliza el proceso de apertura
Then la disputa contiene toda la información necesaria para que pueda ser tratada posteriormente por la DAO
```

## Notas / Referencias al desarrollo

### Referencia Arquitectura

La funcionalidad debe integrarse con:

*   Gestión de proyectos.

*   Gestión de hitos.

*   Sistema de escrow.

*   Sistema de comunicación.

*   DAO de gobernanza.

*   Persistencia de eventos e histórico.


La disputa debe tratarse como una entidad propia, manteniendo referencias al proyecto y al hito que originan la controversia.

La información utilizada para construir el expediente debe proceder del histórico registrado por la plataforma, evitando que el usuario tenga que reconstruir manualmente el contexto del conflicto.

### Estados de la disputa

Para la PoC se propone inicialmente el siguiente ciclo de estados:

```text
OPEN
  |
  v
PUBLIC
  |
  v
PENDING_DAO_RESOLUTION
  |				  |					|
  v				  v					v
RESOLVED		REJECTED		CANCELLED
```

La motivación de los estados de finalización es la siguiente. Los usuarios de la DAO votarán sobre la disputa en función de las siguientes opciones:

*   **Resolver:** `RESOLVED` . Vota a favor de la parte que ha abierto la disputa, considerando que los fundamentos aportados tienen peso suficiente como para considerar que la parte contra la que se abre la disputa no ha cumplido con los términos acordados. En este estado la disputa se resuelve a favor de la parte que ha abierto la disputa.

*   **Rechazar**: `REJECTED`. Vota en contra de la parte que ha abierto la disputa, considerando que los fundamentos aportados **no** tienen peso suficiente como para considerar que la parte contra la que se abre la disputa no ha cumplido con los términos acordados. En este estado la disputa se resuelve en contra de la parte que ha abierto la disputa

*   **Cancelar**: `CANCELLED` o equivalentes queda fuera del alcance de esta DevStory.


### Data Model

#### Datos registrados en Backend

```sql
CREATE TYPE dispute_status_enum AS ENUM (
  'OPEN',
  'PUBLIC',
  'PENDING_DAO_RESOLUTION'
);

CREATE TYPE dispute_argument_type_enum AS ENUM (
  'OPENING',
  'RESPONSE',
  'ADDITIONAL'
);

CREATE TABLE dispute (
  id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  project_id          UUID NOT NULL,
  milestone_id        UUID NOT NULL,
  opened_by_user_id   UUID NOT NULL,
  status              dispute_status_enum NOT NULL DEFAULT 'OPEN',
  opening_reason      TEXT NOT NULL,
  communication_id    UUID,
  created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
  FOREIGN KEY (project_id) REFERENCES project(id),
  FOREIGN KEY (milestone_id) REFERENCES milestone(id),
  FOREIGN KEY (opened_by_user_id) REFERENCES app_user(id)
);

CREATE TABLE dispute_argument (
  id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  dispute_id          UUID NOT NULL,
  user_id             UUID NOT NULL,
  argument_type       dispute_argument_type_enum NOT NULL,
  content             TEXT NOT NULL,
  created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
  FOREIGN KEY (dispute_id) REFERENCES dispute(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES app_user(id)
);

CREATE TABLE dispute_event (
  id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  dispute_id          UUID NOT NULL,
  event_type          VARCHAR(100) NOT NULL,
  event_data          JSONB NOT NULL,
  created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
  FOREIGN KEY (dispute_id) REFERENCES dispute(id) ON DELETE CASCADE
);
```

> Las referencias a `project` y `milestone` representan las entidades existentes en el modelo de datos del proyecto y deberán adaptarse a los nombres definitivos utilizados en la implementación.

### Histórico incluido en la disputa

Al abrir una disputa, el sistema debe conservar una referencia al contexto existente, incluyendo como mínimo:

*   Información del proyecto.

*   Información del hito.

*   Requisitos asociados al hito.

*   Entregables realizados.

*   Versiones de los entregables.

*   Fechas de entrega.

*   Revisiones realizadas.

*   Rechazos anteriores.

*   Motivos de los rechazos.

*   Aceptaciones anteriores, cuando existan.

*   Comunicaciones o eventos relevantes disponibles para la plataforma.


El histórico debe ser **inmutable como evidencia del estado anterior de la disputa**. Las nuevas acciones deben registrarse como nuevos eventos.

### Datos registrados en Blockchain

Para la PoC, la disputa no requiere necesariamente almacenar todo su contenido on-chain.

Como mínimo, deberá evaluarse el registro on-chain de:

*   Identificador de la disputa.

*   Identificador del proyecto/hito.

*   Dirección blockchain de quien abre la disputa.

*   Timestamp de apertura.

*   Hash del expediente o conjunto de evidencias inicial.

*   Estado de la disputa.


El contenido detallado de los argumentos y del histórico puede permanecer off-chain, manteniendo una referencia verificable mediante hash.

### API Reference

#### Backend Endpoints

##### `[POST] /projects/:projectId/milestones/:milestoneId/disputes`

**Descripción:** Abrir una disputa asociada a un hito rechazado.

```json
{
  "reason": "El rechazo no corresponde con los criterios de aceptación acordados para el hito."
}
```

**Respuesta:**

```json
{
  "id": "dispute-uuid",
  "projectId": "project-uuid",
  "milestoneId": "milestone-uuid",
  "status": "OPEN",
  "openedBy": "user-uuid",
  "createdAt": "2026-09-10T12:00:00Z"
}
```

##### `[GET] /disputes/:disputeId`

**Descripción:** Obtener el expediente de una disputa.

```json
{
  "id": "dispute-uuid",
  "projectId": "project-uuid",
  "milestoneId": "milestone-uuid",
  "status": "PUBLIC",
  "openedBy": "user-uuid",
  "openingReason": "El rechazo no corresponde con los criterios acordados.",
  "createdAt": "2026-09-10T12:00:00Z"
}
```

##### `[POST] /disputes/:disputeId/arguments`

**Descripción:** Añadir argumentos al expediente de una disputa.

```json
{
  "content": "El entregable cumple los criterios definidos para este hito.",
  "argumentType": "RESPONSE"
}
```

##### `[GET] /disputes/:disputeId/arguments`

**Descripción:** Obtener los argumentos registrados en una disputa.

```json
[
  {
    "id": "argument-uuid",
    "userId": "user-uuid",
    "argumentType": "OPENING",
    "content": "El rechazo no corresponde con los criterios acordados.",
    "createdAt": "2026-09-10T12:00:00Z"
  }
]
```

##### `[GET] /disputes/:disputeId/history`

**Descripción:** Obtener el histórico de eventos y evidencias asociadas a la disputa.

```json
{
  "project": {},
  "milestone": {},
  "deliverables": [],
  "reviews": [],
  "rejections": [],
  "events": []
}
```

### Eventos

Se recomienda generar eventos de dominio para permitir la evolución posterior hacia un modelo event-driven:

```text
MilestoneRejected
DisputeOpened
DisputeArgumentAdded
DisputeCommunicationChannelCreated
DisputePublished
DisputeReadyForDAO
```

Estos eventos permitirán desacoplar posteriormente:

*   La gestión de disputas.

*   El sistema de comunicación.

*   La gobernanza DAO.

*   La resolución de disputas.

*   Las posibles consecuencias sobre escrow.