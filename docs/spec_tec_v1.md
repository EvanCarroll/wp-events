# The Events Calendar — `tec/v1` REST API: wire-level specification

Derived by reading PHP source (read-only) at
the upstream [the-events-calendar](https://github.com/the-events-calendar/the-events-calendar) repository (TEC `6.18.3.1`, `src/Tribe/Main.php:43`;
bundled common `6.14.0.1`, `common/src/Tribe/Main.php:27`). Endpoints carry `@since 6.15.0`.
Test snapshots in `tests/rest_tec_v1_integration/Endpoints/__snapshots__/*.json` were used to
confirm real response shapes.

Path abbreviations used below:

* `P:` = `src/Events/REST/TEC/V1/` (plugin)
* `C:` = `common/src/Common/REST/TEC/V1/` (framework)
* `R:` = `common/src/Tribe/Repository.php` (ORM base)
* `ER:` = `src/Tribe/Repositories/Event.php` (Event ORM)
* `M:` = `src/Tribe/Models/Post_Types/` (models that produce the TEC-specific response fields)

Marks used: **QUIRK** = the code does something a client would not expect. **DOC≠CODE** = the
OpenAPI documentation the plugin publishes disagrees with what the code actually does.

---

## 1. Namespace, base URL, enablement, gating

### 1.1 Namespace

* Namespace: **`tec/v1`**. Built as `REST_Controller::NAMESPACE . '/v' . self::VERSION`.
  `NAMESPACE = 'tec'` (`common/src/Common/REST/Controller.php:32`), `VERSION = 1`
  (`C:Controller.php:48`), joined at `C:Controller.php:107-109`.
* Base URL: `{site}/wp-json/tec/v1/...`, or with plain permalinks `{site}/?rest_route=/tec/v1/...`
  (standard WP REST).
* Routes are registered on `rest_api_init` (`C:Abstracts/Endpoints_Controller.php:50`) with
  `register_rest_route( 'tec/v1', $path, $methods )` (`C:Abstracts/Endpoint.php:63-69`).

### 1.2 Enablement

* The plugin always registers it. There is no option, constant, or settings toggle.
  * Common: `TEC\Common\Controller::load_controllers()` registers `TEC\Common\REST\Controller` on
    `tribe_common_loaded` (`common/src/Common/Controller.php:52-53`). That registers the V1 controller,
    which registers the `/docs` endpoint (`C:Endpoints.php:42-46`).
  * TEC: `TEC\Events\Controller::get_controllers()` includes `REST_Controller::class`
    (`src/Events/Controller.php:72`) → `TEC\Events\REST\Controller` → `P:Controller.php` →
    `P:Endpoints.php`, which registers `Events, Event, Organizers, Organizer, Venues, Venue`
    (`P:Endpoints.php:47-56`).
  * `Controller_Contract::is_active()` returns `true` unconditionally
    (`common/src/Common/Contracts/Provider/Controller.php:81-83`).
* The minimum version is TEC 6.15.0 (`@since` tags). The `6.15.12` change made create/update use a
  JSON request-body collection (`P:Endpoints/Events.php:352`, `P:Endpoints/Event.php:199`).

### 1.3 Experimental gate (REQUIRED header)

Every TEC entity endpoint (`/events`, `/events/{id}`, `/venues`, `/venues/{id}`, `/organizers`,
`/organizers/{id}`) is **experimental by default**. Only `/docs` is not
(`C:Endpoints/OpenApiDocs.php:278-280` returns `false`).

* `is_experimental()` (`C:Abstracts/Endpoint.php:518-538`) defaults to `true`. Two server-side
  filters can change it:
  * `tec_rest_experimental_{openapi_path}_endpoint`, e.g. `tec_rest_experimental_/events/{id}_endpoint`.
  * `tec_rest_experimental_endpoint`.
  * Nothing in TEC or common sets these to `false`. I searched with `rg` and found no hits.
* Every request to an experimental endpoint, for **every method including GET**, must send:

```
X-TEC-EEA: I understand that this endpoint is experimental and may change in a future release without maintaining backward compatibility. I also understand that I am using this endpoint at my own risk, while support is not provided for it.
```

  * Header name `X-TEC-EEA`, read with `$request->get_header( 'X-TEC-EEA' )`
    (`C:Abstracts/Endpoint.php:487`). Header names are case-insensitive in WP.
  * The value is compared as `strtolower( trim( $header ) ) === strtolower( <sentence> )`
    (`:493`, sentence at `:505-508`). Comparison is case-insensitive, outer whitespace is ignored,
    and the internal text must match exactly (one space after each period, final period included).
* On failure the response is HTTP **400**, a `WP_Error` with code
  `missing_experimental_endpoint_acknowledgement` (`C:Exceptions/ExperimentalEndpointException.php:34-36`):

```json
{"code":"missing_experimental_endpoint_acknowledgement",
 "message":"Experimental endpoint requires acknowledgement header.",
 "data":{"status":400}}
```

  The message is `"Experimental endpoint requires acknowledgement header."` when the header is
  missing or empty, and `"Experimental endpoint requires appropriate acknowledgement header."` when
  the value is wrong (`:490`, `:494`).
* **Ordering QUIRK.** The check runs inside the route callback (`C:Abstracts/Endpoint.php:384-387`).
  WP core runs argument validation and sanitization first, then `permission_callback`, and only then
  the callback. So a request without the header can still get `400 rest_invalid_param`, or
  `401/403 rest_forbidden`, *before* it gets the experimental error.

---

## 2. Routes overview

Path parameter regex: `Positive_Integer` extends `Integer`, so `{id}` becomes `(?P<id>\d+)`
(`C:Abstracts/Endpoint.php:298-322`, `:306-308`).

| Path (OpenAPI form) | WP route regex | Methods | Class | operationId |
|---|---|---|---|---|
| `/docs` | `/docs` | GET | `C:Endpoints/OpenApiDocs.php` | `getOpenApiDocs` |
| `/events` | `/events` | GET, POST | `P:Endpoints/Events.php` | `getEvents`, `createEvent` |
| `/events/{id}` | `/events/(?P<id>\d+)` | GET, PUT, PATCH, DELETE | `P:Endpoints/Event.php` | `getEvent`, `updateEvent`, `deleteEvent` |
| `/venues` | `/venues` | GET, POST | `P:Endpoints/Venues.php` | `getVenues`, `createVenue` |
| `/venues/{id}` | `/venues/(?P<id>\d+)` | GET, PUT, PATCH, DELETE | `P:Endpoints/Venue.php` | `getVenue`, `updateVenue`, `deleteVenue` |
| `/organizers` | `/organizers` | GET, POST | `P:Endpoints/Organizers.php` | `getOrganizers`, `createOrganizer` |
| `/organizers/{id}` | `/organizers/(?P<id>\d+)` | GET, PUT, PATCH, DELETE | `P:Endpoints/Organizer.php` | `getOrganizer`, `updateOrganizer`, `deleteOrganizer` |

* Method constants (`C:Abstracts/Endpoint.php:111-178`):
  * read = `WP_REST_Server::READABLE` (`GET`; WP also answers `HEAD` on GET routes).
  * create = `CREATABLE` (`POST`).
  * update = `self::EDITABLE = 'PUT, PATCH'` (`:45`). **QUIRK:** unlike WP core's `EDITABLE`
    (`POST, PUT, PATCH`), **POST is not accepted on `/{id}`** and returns WP `404 rest_no_route`.
  * delete = `DELETABLE` (`DELETE`).
* `PUT` and `PATCH` are the same handler. Both are partial updates (see §7.3 for injected defaults).
* `OPTIONS` on any route returns WP's standard route description, whose `schema` comes from
  `get_schema()` (`C:Abstracts/Endpoint.php:103`). That is a JSON-Schema draft-04 stub `$ref`-ing
  `{rest_url}/tec/v1/docs#/components/schemas/Event|Venue|Organizer`.
* Ticketing or Pro add-ons (Event Tickets, Events Calendar Pro) may register more `tec/v1` endpoints.
  They are out of scope here.

---

## 3. Authentication and permissions

There is no custom auth. Standard WP REST authentication applies:

* cookie plus `X-WP-Nonce` (or `_wpnonce`);
* Application Passwords via HTTP Basic. The published OpenAPI marks privileged operations with
  `security: [{BasicAuth: []}]`, scheme `{type: http, scheme: basic}` (`C:Documentation.php:228-247`,
  `C:Documentation/OpenAPI_Schema.php:241`).

`permission_callback` per method is in `C:Abstracts/Post_Entity_Endpoint.php`. Capabilities are the
post type's `cap` object.

| Post type | `capability_type` |
|---|---|
| `tribe_events` | `['tribe_event','tribe_events']` (`src/Tribe/Main.php:107`) |
| `tribe_venue` | `['tribe_venue','tribe_venues']` (`src/Tribe/Venue.php:36`) |
| `tribe_organizer` | `['tribe_organizer','tribe_organizers']` (`src/Tribe/Organizer.php:36`) |

All three have `map_meta_cap = true`.

| Operation | Check | Source |
|---|---|---|
| GET collection | `guest_can_read() \|\| current_user_can(cap->read)`. All 3 TEC collections return `guest_can_read() === true`, so this is **always allowed**. | `:59-81`; `P:Endpoints/Events.php:93-95` etc. |
| GET `/{id}` | `get_post(id)` must exist **and** have the endpoint's post type, otherwise **false**. Then `guest_can_read() \|\| current_user_can(cap->read_post, id)`. Then core `WP_REST_Posts_Controller::check_read_permission($post)` (status/visibility). | `:59-77` |
| POST collection | `current_user_can(cap->create_posts)` (e.g. `edit_tribe_events`) | `:92-94` |
| PUT/PATCH `/{id}` | `current_user_can(cap->edit_post, id)` | `:105-112` |
| DELETE `/{id}` | `current_user_can(cap->delete_post, id)` | `:123-130` |
| GET `/docs` | always `true` | `OpenApiDocs.php:57-59` |

If a permission callback returns `false`, WP core returns
`{"code":"rest_forbidden","message":"Sorry, you are not allowed to do that.","data":{"status":401}}`.
The status is 401 when not logged in and 403 when logged in (`rest_authorization_required_code()`).

**QUIRK:** GET `/events/{id}` for a **non-existent ID, or an ID of another post type**, returns
**401/403 `rest_forbidden`**, not 404. The same happens for update and delete on a non-existent ID,
because `map_meta_cap` yields `do_not_allow`. The `{"error":...}` 404 bodies in the traits are only
reached in narrower cases (§9.3).

Silent privilege scale-back on create/update (`C:Abstracts/Post_Entity_Endpoint.php:146-224`):

* `status` (`scale_back_status`, `:170-197`). "Explicitly sent" means the key is present in
  query, form body, or JSON body. Route-arg defaults do not count.
  * Update without an explicit `status`: `status` is removed and left unchanged.
  * User has `cap->publish_posts`: the value is kept. A create without explicit `status` gets the
    default `"publish"`.
  * User lacks `publish_posts` and sent no `status`: forced to `"draft"`.
  * User lacks `publish_posts` and sent `publish`, `future` or `private`: forced to `"pending"`.
* `author` (`scale_back_author`, `:212-224`): if sent and the user lacks `cap->edit_others_posts`,
  it is replaced with the current user ID. No error is returned.

Collection `status` filtering (`C:Traits/Read_Archive_Response.php:121-125`): the status filter is
only applied if the user has `cap->edit_posts`. Otherwise it is **silently forced to `publish`**,
with no error.

---

## 4. Request processing pipeline (order matters for error typing)

For each request:

1. WP core matches the route. If there is no match or the method is wrong: `404 rest_no_route`.
2. WP core applies arg defaults (`default` in the route args), then calls each arg's
   `validate_callback` on the **raw** value, then its `sanitize_callback`.
   * Errors are `400 rest_invalid_param` (§9.1).
   * Route args are `QueryArgumentCollection::to_array()` for GET/DELETE and the flattened body
     properties for POST/PUT/PATCH (`C:Abstracts/Endpoint.php:111-178`;
     `C:Collections/RequestBodyCollection.php:188-239`).
   * Because every arg supplies its own `validate_callback`/`sanitize_callback`, WP's JSON-schema
     validation (`type`, `minimum`, `maximum`, `enum`) is **not** applied. Only TEC's closures run.
3. `permission_callback` (§3) → `401/403 rest_forbidden`.
4. Experimental header check (§1.3) → `400 missing_experimental_endpoint_acknowledgement`.
5. `OpenAPI_Schema::filter_before_request($request->get_params())` (`C:Documentation/OpenAPI_Schema.php:269-343`):
   * **Drops every parameter not declared in the schema** (whitelist).
   * Fills defaults when the value is `empty()`.
   * `intval`s non-body Number/Integer params.
   * Throws `InvalidRestArgumentException` for missing required params (none are required in
     practice), which becomes a 400 `WP_Error` (§9.2).
6. `filter_params()` (status/author scale-back, §3).
7. Handler (read/create/update/delete traits).
8. Response transform (`get_formatted_entity`, §6), then the global WP `rest_post_dispatch`
   handlers (`_fields`, `_envelope`, `_jsonp`).

Parameter source precedence (WP core `WP_REST_Request::get_parameter_order`): JSON body (if
`Content-Type: application/json`), then form body (non-GET), then query string, then URL path
params, then defaults.

**QUIRK:** an `id` key in the JSON body **overrides** the path `{id}`, for both the permission check
and the handler. Never send `id` in a body.

---

## 5. Parameter type semantics (wire coercion)

All types are in `C:Parameter_Types/`. "Validator" is the TEC closure; returning `false` or throwing
causes a 400. "Sanitizer" is what the handler receives.

| TEC type | JSON-schema type/format | Validator | Sanitizer |
|---|---|---|---|
| `Positive_Integer` (`Positive_Integer.php:27-49`) | `integer` | `is_numeric($v) && (int)$v == $v && (int)$v > 0`, else throws `tec_rest_invalid_positive_integer_argument` | `absint($v)` |
| `Integer` (`Integer.php:34-56`) | `integer` | `is_numeric && (int)$v == $v`, else `tec_rest_invalid_integer_argument` | `intval` |
| `Boolean` (`Boolean.php:71-81`) | `boolean` | **accepts anything** | **`(bool) $v`** |
| `Text` (`Text.php:109-158`) | `string` | (a) with `pattern`: `is_string` + `preg_match('/'.pattern.'/')` (`tec_rest_invalid_string_argument`); (b) no `enum`: `is_string($v) && !is_serialized($v)`, returns false (generic error); (c) with `enum`: `in_array($v, enum, true)` (strict, **case-sensitive**) | no enum and no pattern: `sanitize_text_field()`; otherwise `(string)$v` |
| `Date_Time` (`Date_Time.php:27-60`) extends Text | `string`/`date-time` | optional pattern check, then `is_numeric($v) \|\| (is_string($v) && strtotime($v))`, else `tec_rest_invalid_date_time_argument` | as Text: `sanitize_text_field` if no pattern, raw string if pattern |
| `Date` (`Date.php`) extends Date_Time | `string`/`date` | same | same |
| `URI` (`URI.php:34-49`) extends Text | `string`/`uri` | `filter_var($v, FILTER_VALIDATE_URL)`, else `tec_rest_invalid_uri_argument` | `sanitize_text_field` |
| `Email` (`Email.php:34-49`) extends Text | `string`/`email` | `is_email($v)`, else `tec_rest_invalid_email_argument` | `sanitize_text_field` |
| `Array_Of_Type` (`Array_Of_Type.php:104-165`) | `array` | default: value must be a PHP **array** or a **string containing `,`**. Strings are split on `,` and each item goes through the item type's validator. Some params use a custom validator (noted per param). | string containing `,` is exploded, otherwise `(array)$v`; each item goes through the item sanitizer |

Notes on the table:

* `Boolean` **QUIRK:** `(bool) $v` means a query string `?featured=false` is `"false"`, which is
  `true`. Use `0`, `""`, or omit the key for false, and `1` for true. JSON `true`/`false` work as
  expected.
* `Text` **QUIRK:** `sanitize_text_field` strips HTML tags, line breaks, tabs and extra whitespace.
  This applies to body `content`, `excerpt`, `title`, `cost`, etc.
* `URI` / `Email` **QUIRK:** an empty string `""` fails validation, so you cannot clear a URL or
  email field by sending `""`.
* `Array_Of_Type` **QUIRK (query strings):** a single scalar without a comma, e.g. `?categories=5`,
  **fails** default validation. Use `categories[]=5` or `categories=5,6`. A trailing comma (`5,`)
  produces an empty item, which fails the item validator.
* Custom array validators (`status`, `venue`, `organizer`) accept a single scalar.

Integer-like path and query values are additionally `intval`ed in `filter_before_request`
(`OpenAPI_Schema.php:320-325`). Body values are not.

Error messages literally contain braces, e.g. ``Argument `{per_page}` must be a positive integer.``
(**QUIRK**).

---

## 6. Response entity shapes

### 6.1 How an entity is built

`C:Abstracts/Post_Entity_Endpoint.php:291-352`:

1. `new WP_REST_Posts_Controller($post_type)->prepare_item_for_response($post, $request)`. This is
   the **WP core post response** for the current `context` (`view` by default).
   * `context=edit` is honoured only if the user has `edit_post` on that specific post (`:300-303`).
     Otherwise it is silently downgraded to `view`.
   * `context` is not a declared param, but `get_formatted_entity` reads the raw request, so
     `?context=edit` works.
2. `prepare_response_for_collection()`, which drops links.
3. `add_properties_to_model()` (`:322-352`):
   * Adds the model's extra fields from `{Model}::get_properties_to_add()` (filterable:
     `tec_rest_event_properties_to_add`, `tec_rest_venue_properties_to_add`,
     `tec_rest_organizer_properties_to_add`).
   * Sets `link = permalink ?? link`.
   * **Removes `permalink`, `meta`, `_links`.**
4. `transform_entity()`:
   * For events, `organizers` and `venues` are replaced with fully formatted Organizer/Venue
     entities. Linked posts the current user cannot read (core `check_read_permission`) are
     **silently dropped** (`P:Traits/With_Transform_Organizers_And_Venues.php:35-46`;
     `format_entity_collection` `:267-279`).
   * Filters run: `tec_rest_v1_{post_type}_transform_entity`, then `tec_rest_v1_post_entity_transform`.
   * The latter triggers password hiding (§6.7).

Field order on the wire: core fields first, then TEC fields in `get_properties_to_add()` order (with
`permalink` removed). There is **no `_links`** and therefore `_embed` does nothing. The global
`?_fields=a,b` (WP core `rest_filter_response_fields`) and `?_envelope` still work.

### 6.2 Core (WP) fields: `TEC_Post_Entity`

Present on Event, Venue and Organizer. They come from `WP_REST_Posts_Controller`. Which ones appear
depends on post-type supports, viewability and taxonomies. Documented in
`C:Documentation/TEC_Post_Entity_Definition.php:49-273`; actual presence confirmed by snapshots.

| Field | Type (view ctx) | Null? | Present on | Notes |
|---|---|---|---|---|
| `id` | integer | no | all | |
| `date` | string `YYYY-MM-DDTHH:MM:SS` (site tz, **no offset**) | doc: nullable | all | core `mysql_to_rfc3339`. Doc pattern allows `T`/space, fractional seconds, offset. Treat as optional-offset RFC3339-ish. |
| `date_gmt` | string, same format (UTC, no `Z`) | doc: nullable | all | |
| `guid` | object `{ "rendered": string }` | doc: nullable | all | edit ctx adds `raw: string` |
| `modified` | string, same format | no | all | |
| `modified_gmt` | string, same format | no | all | |
| `slug` | string | no | all | may be `""` for drafts (snapshot) |
| `status` | string enum `publish\|future\|draft\|pending\|private` | no | all | single reads use ORM `status=any`, which excludes `trash`/`auto-draft` |
| `type` | string: `"tribe_events"` \| `"tribe_venue"` \| `"tribe_organizer"` | no | all | **not documented** in the TEC OpenAPI |
| `link` | string (URL) | no* | all | Replaced by the model `permalink` (`:344`). *Venue/Organizer `get_permalink()` can return `false`, so `link` may be `false` (edge case). Drafts get `?post_type=...&p=ID` URLs. |
| `title` | object `{ "rendered": string }` | no | all | HTML-entity-encoded (`&#8217;`, `&#038;`). Edit ctx adds `raw`. |
| `content` | object `{ "rendered": string, "protected": bool }` | `protected` may be `null` (§6.7) | all | edit ctx adds `raw: string`, `block_version: int` |
| `excerpt` | object `{ "rendered": string, "protected": bool }` | same | all (snapshots show it on venue/organizer too) | edit ctx adds `raw` |
| `author` | integer | no | all (per snapshots) | |
| `featured_media` | integer (0 = none) | no | all (per snapshots) | |
| `template` | string (`""` default) | no | all | |
| `tags` | integer[] (`post_tag` term IDs) | no | **Event only** | |
| `tribe_events_cat` | integer[] (term IDs) | no | **Event only** | Documented in `Event_Definition` (`P:Documentation/Event_Definition.php:64-70`) |
| `class_list` | string[] | no | **Event only** | Core adds it for viewable post types. Venues/organizers have `public => false` (`src/Tribe/Venue.php:28`, `src/Tribe/Organizer.php:31`). **Not documented.** |
| `permalink_template` | string | – | edit ctx, viewable types | documented `readOnly` |
| `generated_slug` | string | – | edit ctx, viewable types | documented `readOnly` |
| `password` | string | – | edit ctx only | core |

Documented but normally **absent** (event/venue/organizer post types do not support them):
`comment_status`, `ping_status`, `format`, `sticky` (core's sticky is `post`-only; the Event `sticky`
is the TEC field, see §6.3), `menu_order`, `parent`. `meta` and `_links` are always stripped.

Extra taxonomies (e.g. from Pro) would appear as additional `<rest_base>: int[]` fields. Use
`#[serde(flatten)] extra: Map<String, Value>` to tolerate them.

### 6.3 Event: `allOf [TEC_Post_Entity, Event]`

The OpenAPI is in `P:Documentation/Event_Definition.php:61-286`. The actual keys come from
`M:Event.php:400-437` (`get_properties_to_add`) and their values from `M:Event.php` `build_properties`
(`:40-~330`).

Actual key order observed:

```
id, date, date_gmt, guid, modified, modified_gmt, slug, status, type, link, title, content, excerpt,
author, featured_media, template, tags, tribe_events_cat, class_list, start_date, start_date_utc,
end_date, end_date_utc, dates, timezone, duration, multiday, is_past, is_now, all_day,
starts_this_week, ends_this_week, this_week_duration, happens_this_week, displays_on, featured,
sticky, cost, organizer_names, organizers, venues, schedule_details, short_schedule_details
```

| Field | Actual wire type | Nullable | Doc type | Notes |
|---|---|---|---|---|
| `start_date` | string `YYYY-MM-DD HH:MM:SS` (event local tz) | yes (null if meta missing) | date-time, pattern `^[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}$` | Space separator, no tz. Raw `_EventStartDate` meta. |
| `start_date_utc` | string, same format, UTC | yes | same | `_EventStartDateUTC` |
| `end_date` | string, same format | yes | same | |
| `end_date_utc` | string, same format | yes | same | |
| `dates` | object `DateDetails` (§6.6) | no | `$ref DateDetails` | 8 PHP `DateTimeImmutable` JSON objects |
| `timezone` | string (IANA, e.g. `"Europe/Athens"`, or `"UTC"`, or `"UTC+2"`-style) | no | string | `Timezones::get_event_timezone_string` |
| `duration` | **string of digits** (e.g. `"28800"`), or **integer** when recomputed | no | Positive_Integer | **DOC≠CODE.** `(int) isset(...) ? meta : null` casts the `isset()` result, not the value (`M:Event.php` build_properties). The raw meta string is returned; an int is returned only if the meta is missing or empty. Seconds. Rust: deserialize `string\|int` to `u64`. |
| `multiday` | **`false` or positive integer** (number of days spanned) | no | boolean | **DOC≠CODE.** All-day events: `$start_end_diff->days` or `false`; otherwise the count is ≥ 2 or `false`. Rust: `enum { No(false), Days(u32) }`, or `Option<u32>` with a custom deserializer. |
| `is_past` | bool | no | bool | start < now |
| `is_now` | bool | no | bool | |
| `all_day` | bool | no | bool | |
| `starts_this_week` | bool | **yes; always `null` in REST** | bool, nullable | Only computed when the model filter is a date. The REST ORM uses `raw`. |
| `ends_this_week` | bool | yes, always null | bool, nullable | |
| `this_week_duration` | integer (1..7) | yes, always null | Positive_Integer, nullable | |
| `happens_this_week` | bool | yes, always null | bool, nullable | |
| `displays_on` | string[] (`YYYY-MM-DD`) | no | `Date[]` | Always `[]` in REST, for the same reason. |
| `featured` | bool | no | bool | `_tribe_featured` meta truthy |
| `sticky` | bool | no | bool | `menu_order === -1` |
| `cost` | string (may be `""`; formatted with currency, e.g. `"$99.00"`, `"Free"`) | no | string | `tribe_get_cost($id, true)` |
| `organizer_names` | string[] | no | string[] | Names of **all** linked organizers, including ones dropped from `organizers` by permission. |
| `organizers` | `Organizer[]` (§6.5) | no (may be `[]`) | `Organizer[]` | Full entities, permission-filtered |
| `venues` | `Venue[]` (§6.4) | no (may be `[]`) | `Venue[]` | Full entities, permission-filtered |
| `schedule_details` | string (**HTML**, e.g. `<span class="tribe-event-date-start">…</span>`) | no | string | |
| `short_schedule_details` | string (HTML) | no | string | |
| `ticketed` | `string[]` of provider slugs **or `false`** | – | `Text[]` (doc says "or false") | **Not in `get_properties_to_add()`.** Absent unless Event Tickets adds it via `tec_rest_event_properties_to_add`. Treat as `Option<Ticketed>`. |

Other notes:

* If the model's `build_properties` throws, it returns `[]`, and **all TEC-specific fields are
  absent**. Defensive clients should make every TEC field `Option`/`#[serde(default)]`.
* Event model fields that are *not* emitted: `excerpt` (the core one is used), `title`, `thumbnail`,
  `plain_schedule_details`, `permalink` (moved into `link`).

### 6.4 Venue: `allOf [TEC_Post_Entity, Venue]`

* Keys: `M:Venue.php:73-100`. Values: `M:Venue.php` build_properties. Doc:
  `P:Documentation/Venue_Definition.php:62-125`.
* Order: core fields (no `tags`, `tribe_events_cat`, `class_list`), then
  `address, country, city, state_province, state, province, zip, phone, directions_link, website`.

| Field | Wire type | Notes |
|---|---|---|
| `address` | string | `""` if unset |
| `country` | string | |
| `city` | string | |
| `state_province` | string | `_VenueStateProvince` |
| `state` | string | |
| `province` | string | |
| `zip` | string | |
| `phone` | string | |
| `directions_link` | string, may be `""` | Doc: `format: uri`. **QUIRK:** HTML-escaped ampersands (`&#038;`), e.g. `https://maps.google.com/maps?f=q&#038;source=s_q&#038;...`. Unescape before use. Do not type as `Url`. |
| `website` | string, may be `""` | Doc: `format: uri`. Do not type as `Url` (empty string). |

### 6.5 Organizer: `allOf [TEC_Post_Entity, Organizer]`

* Keys: `M:Organizer.php:65-85`. Doc: `P:Documentation/Organizer_Definition.php:62-76`.
* Order: core fields, then `phone, website, email`.

| Field | Wire type | Notes |
|---|---|---|
| `phone` | string (may be `""`, free text e.g. `"+1-555-BOOK-NOW"`) | doc `format: tel` |
| `website` | string (may be `""`) | doc `format: uri` |
| `email` | string (may be `""`; may hold several comma-separated addresses) | doc `format: email`. Not antispambot-mangled. |

### 6.6 `DateDetails` and `Date` (inside `Event.dates`)

`C:Documentation/Date_Details_Definition.php:55-97`, `C:Documentation/Date_Definition.php:57-111`.

`DateDetails` is an object with **exactly these 8 keys**, each a `Date`:
`start, start_utc, start_site, start_display, end, end_utc, end_site, end_display`.

* `*_utc` values are in UTC.
* `*_site` values are in the site timezone.
* `*_display` values equal the event-tz value when the "event timezone" mode is on, else the site-tz
  value.

`Date` is PHP's native `DateTimeImmutable` JSON serialization:

| Field | Type | Notes |
|---|---|---|
| `date` | string `YYYY-MM-DD HH:MM:SS.ffffff` (always 6 fraction digits) | pattern `^[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}$`, required |
| `timezone_type` | integer: `1` (UTC offset like `+02:00`), `2` (abbreviation like `EST`), `3` (identifier like `Europe/Athens`) | required. **`0` when password-hidden** (§6.7). |
| `timezone` | string | required |

### 6.7 Password-protected posts: response mangling

`C:Controller.php:138-167, 293-336`. This runs on every entity via `tec_rest_v1_post_entity_transform`
at priority 10000.

If `post_password_required($post)` is true (filterable: `tec_rest_v1_post_password_required`), every
key **except** the whitelist (`C:Controller.php:57-71`) is rewritten recursively.

Whitelist: `id, date, date_gmt, guid, modified, modified_gmt, slug, status, type, link, title,
author, class_list`. The check is per key and applies at every nesting level, so a nested `date` key
(inside `Date`) is also kept.

| Original value | Becomes |
|---|---|
| string | `"Password protected"` (translatable, `__('Password protected','tribe-common')`) |
| int/float/numeric | `0` |
| bool / null / other scalar | **`null`** |
| callable | `null` |
| `DateTimeImmutable` | replaced by `1970-01-01 00:00:00 UTC`, then recursed. Result: `{"date":"1970-01-01 00:00:00.000000","timezone_type":0,"timezone":"Password protected"}` |
| array/object | recursed (lists keep length; e.g. `organizer_names` becomes `["Password protected", …]`) |

Consequences for typing:

* Any bool field becomes nullable, e.g. `content.protected: null`, `featured: null`, `all_day: null`.
* Any string field can be `"Password protected"`.
* `duration` becomes `"Password protected"`.
* `featured_media` and `this_week_duration` become `0`.
* Snapshot evidence: `Event_Test__test_get_formatted_entity__0.snapshot.json`.

Unlocking:

* Single GET only. Send `?password=<post password>`, or use `context=edit` with `edit_post`
  capability. If `can_access_password_content()` passes, `post_password_required` is bypassed for
  that request (`C:Traits/Read_Entity_Response.php:66-81`) and the data is returned in full.
* Collections never unlock.
* **Even editors** see hidden data in collections, because `post_password_required` is cookie-based,
  not capability-based.

---

## 7. Per-route specification

All entity routes require the `X-TEC-EEA` header (§1.3). Query param names are exact. "Default"
means injected when absent or `empty()`.

### 7.1 `GET /tec/v1/events`: list events

`P:Endpoints/Events.php:216-346` (read_params); handler `C:Traits/Read_Archive_Response.php:35-138`.

| Param | Type | Default | Constraints | Server effect (ORM `tribe_events()`) |
|---|---|---|---|---|
| `page` (`:219`) | Positive_Integer | `1` | ≥1 (validator). Doc `minimum: 1`. | `->page()` |
| `per_page` (`:226`) | Positive_Integer | `get_option('posts_per_page')` (filter `tec_rest_events_default_per_page`), usually 10 | doc `minimum 1, maximum 100`. **Max NOT enforced** (validator only checks >0; `get_max_posts_per_page()` is unused). | `->per_page()` |
| `start_date` (`:234`) | Date_Time (no pattern) | – | any `strtotime()`-parsable string or a numeric timestamp | `start_date` → `filter_by_starts_on_or_after` (**inclusive**; doc says "after") (`ER:183`) |
| `end_date` (`:239`) | Date_Time | – | same | `end_date` → `filter_by_ends_on_or_before` (inclusive) (`ER:184`) |
| `search` (`:244`) | Text (sanitize_text_field) | – | – | `->search()` → WP `s` |
| `categories` (`:249`) | Array_Of_Type<Positive_Integer> (default validator: array or comma string) | – | items >0 | **QUIRK: no-op.** The key `categories` is not an Event ORM filter, so it falls through as an unknown `WP_Query` var (`R:1230-1243`). Still validated. |
| `tags` (`:255`) | Array_Of_Type<Positive_Integer> | – | – | **QUIRK: no-op** (ORM knows `tag`, not `tags`) |
| `venue` (`:261`) | Array_Of_Type<Positive_Integer>, custom validator `is_venue_id_list` (`src/Tribe/Validator/Base.php:227-234`): every ID must be an existing venue; single scalar OK | – | – | `filter_by_venue` → `meta_in _EventVenueID` (`ER:918`) |
| `organizer` (`:270`) | same, `is_organizer_id_list` (`Base.php:55-62`) | – | – | `filter_by_organizer` → `meta_in _EventOrganizerID` |
| `featured` (`:279`) | Boolean | – (absent = no filter) | – | `true`: only featured; **`false` (e.g. `featured=0`): only non-featured** (`ER:851-853`). `featured=false` in a query string means TRUE (§5). |
| `status` (`:284`) | Array_Of_Type<Text enum>, enum `publish,pending,draft,future,private` (`C:Abstracts/Post_Entity_Endpoint.php:37`), custom validator `validate_status` (`:363-376`; comma string or array, all items in enum) | `["publish"]` | – | Applied only if the user has `edit_tribe_events`, else forced to `publish` (§3) |
| `post_parent` (`:293`) | Positive_Integer | – | – | WP `post_parent` |
| `starts_before` (`:298`) | Date_Time | – | – | `filter_by_starts_before` (exclusive) |
| `starts_after` (`:303`) | Date_Time | – | – | `filter_by_starts_after` (exclusive) |
| `ends_before` (`:308`) | Date_Time | – | – | `filter_by_ends_before` |
| `ends_after` (`:313`) | Date_Time | – | – | `filter_by_ends_after` |
| `ticketed` (`:318`) | Boolean | – | – | **No-op in TEC alone.** No `ticketed` ORM filter exists in TEC; Event Tickets may supply one. |
| `orderby` (`:323`) | Text enum `date \| event_date \| title \| menu_order \| modified` (strict, case-sensitive) | `"event_date"` | – | `->order_by(orderby, order)`, applied only if both are non-empty (always, given defaults) |
| `order` (`:330`) | Text enum `ASC \| DESC` (**uppercase only**; `asc` → 400) | `"ASC"` | – | |

* The list is filterable server-side: `tec_events_rest_v1_events_read_params` (`:345`).
* Undeclared params are **silently dropped** (§4 step 5). Exceptions:
  * `context`, `password` and `_fields`-style globals are read from the raw request elsewhere.
  * `tec_events_ignore=true` is always applied server-side (`Read_Archive_Response.php:43`;
    custom-tables bypass).

Responses:

* `200`: JSON array of Event (§6.3), plus pagination headers (§8).
* `400`: invalid param (§9.1).
* `404` when the page is out of range (§8).

### 7.2 `POST /tec/v1/events`: create event

Handler `C:Traits/Create_Entity_Response.php:38-63`. Body schema is `Event_Request_Body` =
`allOf [TEC_Post_Entity_Request_Body, Event_Request_Body]`.

* Send it as JSON (`Content-Type: application/json`). Form-encoded and query-string params are also
  accepted by WP, but JSON is the documented content type.
* There are no required fields at the API layer. The ORM requires `title` and an end date
  (explicit, or derivable from start + duration), else **500** (§7.8).

Common body fields (`C:Documentation/TEC_Post_Entity_Request_Body_Definition.php:47-164`):

| Field | Type | Default (injected!) | Constraints | ORM mapping (`R:388-402`) |
|---|---|---|---|---|
| `date` | Date_Time, pattern `^\d{4}-\d{2}-\d{2}[Tt ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z\|[+-]\d{2}(?::\d{2})?)?$`; doc nullable | – | must also `strtotime` | `post_date` |
| `date_gmt` | same | – | | `post_date_gmt` |
| `slug` | Text (sanitized) | – | | `post_name` |
| `status` | Text enum `publish,pending,draft,future,private` | **`"publish"`** | strict | `post_status` (after scale-back, §3) |
| `title` | Text (sanitize_text_field → **HTML stripped**) | – | | `post_title` |
| `content` | Text (**sanitize_text_field → HTML tags and newlines stripped**) | – | | `post_content` |
| `excerpt` | Text (sanitized) | – | | `post_excerpt` |
| `author` | Positive_Integer | – | | `post_author` (scale-back, §3) |
| `featured_media` | Positive_Integer | – | | **QUIRK:** no alias; ends up as post **meta key `featured_media`**, not `_thumbnail_id`. The thumbnail is not set. |
| `comment_status` | Text enum `open\|closed` | **`"open"`** | | `comment_status` post field |
| `ping_status` | Text enum `open\|closed` | **`"open"`** | | `ping_status` post field |
| `format` | Text enum `standard,aside,chat,gallery,link,image,quote,status,video,audio` | **`"standard"`** | | **QUIRK:** stored as meta key `format` (no effect on post format) |
| `sticky` | Boolean | **`false`** (from the common definition; see §7.3) | | Events: truthy → `menu_order=-1`, else `0` (`ER:1492-1498`) |
| `template` | Text | – | | **QUIRK:** stored as meta key `template` (not `_wp_page_template`) |
| `tags` | Array_Of_Type<Positive_Integer> | – | array or comma string | Events: `post_tag` term IDs (alias `ER:127`) |

Event-specific body fields (`P:Documentation/Event_Request_Body_Definition.php:57-152`; ORM aliases
`ER:111-138`):

| Field | Type | Constraints | ORM mapping |
|---|---|---|---|
| `tribe_events_cat` | Array_Of_Type<Positive_Integer> | array or comma string | `tribe_events_cat` taxonomy (term IDs) |
| `start_date` | Date_Time, pattern `^[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}$` | **exact `YYYY-MM-DD HH:MM:SS`**, event-local | `_EventStartDate`. UTC is derived. If both local and UTC are given, local wins (`ER:1174-1202`). |
| `start_date_utc` | same pattern | | `_EventStartDateUTC` (used only if local is not given) |
| `end_date` | same pattern | | `_EventEndDate` |
| `end_date_utc` | same pattern | | `_EventEndDateUTC` |
| `timezone` | Text | e.g. `Europe/Athens`; `""` means site tz | `_EventTimezone` |
| `duration` | Positive_Integer (seconds) | | `_EventDuration`. If start and end both change, duration is recomputed. With duration + start, end is derived. |
| `all_day` | Boolean | | `_EventAllDay="yes"` if truthy. **QUIRK:** a falsy value is *unset* from the update, so sending `false` **does not clear** all-day (`ER:1148-1155`). |
| `featured` | Boolean | | `_tribe_featured` |
| `sticky` | Boolean (no default in this definition) | | see above |
| `cost` | Text (sanitized) | | `_EventCost` |
| `organizers` | Array_Of_Type<Positive_Integer> | **must be a JSON array** (a scalar fails); items >0 | `_EventOrganizerID` (multiple). Unlinkable or unreadable IDs are silently skipped (`ER:1341-...`). |
| `venues` | Array_Of_Type<Positive_Integer> | must be an array | `_EventVenueID`. If venues do not `allow_multiple`, only the first is used. |

Date sanity: if end < start, all date fields are silently dropped (`ER` update_date_meta). On create
that makes the ORM reject the post (no end date), giving **500**.

Responses:

* `201` with the created Event (§6.3), re-read via ORM `status=any`.
* `400` validation.
* `401`/`403` permission.
* `500` `{"error":"Failed to create entity."}`.

### 7.3 `PUT|PATCH /tec/v1/events/{id}`: update event

Handler `C:Traits/Update_Entity_Response.php:36-102`. Same body as create (route args are the
flattened `Event_Request_Body` props; when a name is declared twice, the later Event-level
declaration overrides the arg).

* Path `id`: Positive_Integer.
* Partial update: only keys present are written, **except injected defaults**.

**QUIRK, injected defaults on update.** WP route-arg defaults and `filter_before_request` default
filling both apply on update. So when they are *omitted*, these values are written:

* `comment_status="open"`
* `ping_status="open"`
* `format="standard"` (meta)
* `sticky=false`. Events: `menu_order=0`, **unsticking the event**. Venues/organizers: meta `sticky`.

`status` is protected: if not explicitly sent on update, it is removed (§3). To preserve stickiness
or comment status, always send `sticky` / `comment_status` explicitly. (Inferred from code; verify
against a live site if it matters.)

The ORM updates by `id` with `status=any` (excludes `trash`). Updating a **trashed** post returns
`500 {"error":"Failed to update entity."}`.

Responses:

* `200` with the updated Event.
* `400`, `401`, `403`.
* `404 {"error":"The entity could not be updated."}` (id falsy, or post type mismatch).
* `500 {"error":"Failed to update entity."}` (save returned empty).
* `500 {"error":"Entity not found after update."}`.

### 7.4 `GET /tec/v1/events/{id}`: read event

* No query params are declared (`P:Endpoints/Event.php:156-158`). Raw `context` and `password` are
  honoured (§6.1, §6.7).
* Handler `C:Traits/Read_Entity_Response.php:39-107`.
* Responses:
  * `200` Event.
  * `401/403 rest_forbidden` for nonexistent, wrong-type or unreadable IDs (§3).
  * `404 {"error":"The entity could not be read."}` when the post type matches but the ORM finds
    nothing (e.g. a trashed post read by an editor).

### 7.5 `DELETE /tec/v1/events/{id}`: delete event

Params (`C:Abstracts/Post_Entity_Endpoint.php:233-244`):

| Param | Type | Default |
|---|---|---|
| `force` | Boolean (query or body) | `false` |

Semantics (`C:Traits/Delete_Entity_Response.php:33-146`), in order:

1. Missing id → `404 {"error":"The entity could not be deleted."}`.
2. `get_post_type(id) !== endpoint post type` → `404 {"error":"The entity could not be deleted."}`.
3. `get_post(id)` is null → `404 {"error":"The entity could not be found."}`.
4. `force` truthy → `wp_delete_post(id, true)`.
   * Success: **`200` with body `[]`** (an empty JSON *array*).
   * Failure: `500 {"error":"Force deletion failed - the entity could not be permanently deleted."}`.
   * This works on posts in any status, including trashed ones.
5. Not forced and `EMPTY_TRASH_DAYS <= 0` →
   `501 {"error":"The entity does not support trashing. Set 'force=true' to delete."}`.
6. Already `trash`:
   * If the filter `tec_rest_delete_convert_trashed_to_permanent` (default `false`) is true:
     permanent delete, giving `200 []`, or
     `500 {"error":"The entity could not be permanently deleted."}`.
   * Otherwise: `410 {"error":"The entity has already been trashed."}`.
7. Otherwise `wp_trash_post(id)`: `200 []`, or `500 {"error":"The entity could not be trashed."}`.

The response never contains the deleted entity (unlike WP core, which returns
`{deleted, previous}`).

**DANGER QUIRK:** `DELETE ...?force=false` sends the string `"false"`, which becomes `(bool)"false"`
= **true**, a permanent delete. Send `force=0`, omit it, or use a JSON body `{"force": false}`.

Permission is `delete_post` on the ID. A nonexistent ID gives `401/403`.

### 7.6 Venues: `GET|POST /tec/v1/venues`, `GET|PUT|PATCH|DELETE /tec/v1/venues/{id}`

Collection GET params (`P:Endpoints/Venues.php:143-197`; filter `tec_events_rest_v1_venues_read_params`):

| Param | Type | Default | Effect (ORM `tribe_venues()`) |
|---|---|---|---|
| `page` (`:145`) | Positive_Integer | `1` | |
| `per_page` (`:152`) | Positive_Integer | posts_per_page | doc max 100, not enforced |
| `search` (`:160`) | Text | – | WP `s` |
| `event` (`:165`) | Positive_Integer | – | `filter_by_event`: venues linked to that event (`src/Tribe/Repositories/Linked_Posts.php:58-...`) |
| `has_events` (`:170`) | Boolean | – | **QUIRK:** `filter_by_has_events()` takes no argument, so **any** value, including `false`/`0`, restricts to venues that have events (`src/Tribe/Repositories/Venue.php:130-143`). Omit to disable. |
| `only_with_upcoming` (`:175`) | Boolean | – | **QUIRK: no-op** (no ORM filter of that name) |
| `status` (`:180`) | Array_Of_Type<Text enum> (as events) | `["publish"]` | same scale-back as events (needs `edit_tribe_venues`) |

* There is **no `orderby`/`order`**; anything sent is dropped. Ordering is WP_Query's default:
  `post_date DESC`.
* Venue body (`P:Documentation/Venue_Request_Body_Definition.php:59-115`) = common fields (§7.2)
  plus the fields below. ORM aliases are in `src/Tribe/Repositories/Venue.php:42-60`.

| Field | Type | Maps to |
|---|---|---|
| `address` | Text | `_VenueAddress` |
| `country` | Text | `_VenueCountry` |
| `city` | Text | `_VenueCity` |
| `state_province` | Text | `_VenueStateProvince` |
| `state` | Text | `_VenueState` |
| `province` | Text | `_VenueProvince` |
| `zip` | Text | `_VenueZip` |
| `phone` | Text | `_VenuePhone` |
| `website` | URI (`FILTER_VALIDATE_URL`; `""` rejected) | `_VenueURL` |

* `directions_link` is read-only (it is not in the request body).
* The common `tags` field on venues is stored as meta `tags` (no alias). **QUIRK.**
* Create requires `title` (ORM `filter_postarr_for_create`, `src/Tribe/Repositories/Venue.php:114-121`),
  else `500 {"error":"Failed to create entity."}`.
* Single read, update and delete status codes are identical to events (§7.3–7.5). The OpenAPI lists
  the same response codes (`P:Endpoints/Venue.php`).

### 7.7 Organizers: `GET|POST /tec/v1/organizers`, `GET|PUT|PATCH|DELETE /tec/v1/organizers/{id}`

* Collection GET params are identical to venues (`P:Endpoints/Organizers.php:142-188`): `page`,
  `per_page`, `search`, `event`, `has_events` (same any-value quirk,
  `src/Tribe/Repositories/Organizer.php:81-94`), `only_with_upcoming` (no-op), `status`.
* Unlike venues and events, the organizer params are **not** passed through an `apply_filters`
  (`:188` returns `$collection` directly).
* Organizer body (`P:Documentation/Organizer_Request_Body_Definition.php:60-74`) = common fields plus:

| Field | Type | Maps to (`src/Tribe/Repositories/Organizer.php:42-47`) |
|---|---|---|
| `phone` | Text (doc `format: tel`, not validated as tel) | `_OrganizerPhone` |
| `website` | URI (`""` rejected) | `_OrganizerWebsite` |
| `email` | Email (`is_email`; `""` rejected; multiple addresses rejected) | `_OrganizerEmail` |

Create requires `title` (`src/Tribe/Repositories/Organizer.php:67-74`).

### 7.8 `GET /tec/v1/docs`: OpenAPI document

* `C:Endpoints/OpenApiDocs.php`. No auth, **not experimental** (no header needed).
* Returns an OpenAPI **3.0.4** document (`C:Documentation.php:35, 80-106`):
  * `info.version` is `"1.0.0"`.
  * `servers[0].url` is `rest_url('tec/v1')`.
  * `components.schemas` keys: `OpenApiDocumentation`, `OpenApiPath`, `DateDetails`, `Date`, `TEC_Post_Entity`,
    `TEC_Post_Entity_Request_Body`, `Event`, `Organizer`, `Venue`, `Event_Request_Body`,
    `Organizer_Request_Body`, `Venue_Request_Body`.
  * `securitySchemes.BasicAuth`.
  * `tags`: `Common` and `Events`.
* Filterable server-side: `tec_rest_v1_openapi_docs`, `tec_rest_swagger_documentation`,
  `tec_rest_swagger_{type}_definition`.
* This document is the "intended" contract. Where it disagrees with the code, this spec marks
  **DOC≠CODE**. Do not generate types blindly from it.

---

## 8. Pagination (collections)

`C:Traits/Read_Archive_Response.php:35-86`.

* The body is a **bare JSON array** of entities. There is no wrapper object.
* Response headers:
  * `X-WP-Total`: integer, total matching items (`$query->found()`).
  * `X-WP-TotalPages`: `ceil(total / per_page)`. **`0` when total is 0** (**QUIRK**; WP core
    returns ≥1 in some cases).
  * `Link`: present only if there is a next and/or previous page.
    * Built with `WP_REST_Response::link_header()`. The format is `<URL>; rel="next"`, and if both
      exist they are comma-joined into one header value, **next first, then prev**:
      `<…page=3…>; rel="next", <…page=1…>; rel="prev"`.
    * The URL is `rest_url(route)` with **all original query params** re-added via `add_query_arg`
      and `page` replaced. `add_query_arg` does not re-urlencode values, so parse leniently.
    * Omitted on single-page results.
* Out-of-range page: when the result is empty **and** `page > 1`, the response is HTTP **404** with
  body (`:49-56`):

```json
{"code":"tec_rest_organizers_page_not_found","message":"The requested page was not found."}
```

  **QUIRK:** the code is hardcoded `tec_rest_organizers_page_not_found` even for events and venues,
  there is no `data` member, and no pagination headers are sent.
* An empty `page=1` result is `200 []` with `X-WP-Total: 0`, `X-WP-TotalPages: 0`.
* Post-processing filter: `tec_rest_{post_type}_archive`, e.g. `tec_rest_tribe_events_archive`
  (`:67`). It can change the body after the total is counted.
* Single-entity responses have no pagination headers.

---

## 9. Error responses

There are **three distinct error body shapes**. A Rust client should model them as an untagged enum.

### 9.1 WP core `WP_Error` shape (validation, permission, routing)

```json
{"code": "<string>", "message": "<string>", "data": {"status": <int>, ...}}
```

* `rest_invalid_param` (400). Raised when a `validate_callback` returns `false` or a `WP_Error`.

```json
{"code":"rest_invalid_param","message":"Invalid parameter(s): per_page",
 "data":{"status":400,
   "params":{"per_page":"Argument `{per_page}` must be a positive integer."},
   "details":{"per_page":{"code":"tec_rest_invalid_positive_integer_argument",
       "message":"Argument `{per_page}` must be a positive integer.",
       "data":{"status":400,"argument":"per_page","details":"The argument `{per_page}` is not a positive integer."}}}}}
```

  * When a validator returns plain `false` (Text without enum/pattern, Array_Of_Type default,
    `status`, `venue`, `organizer`, enum mismatch), `params[name] = "Invalid parameter."` and there
    is **no** `details[name]` entry.
  * TEC inner codes (in `details[*].code`):
    * `tec_rest_invalid_positive_integer_argument`
    * `tec_rest_invalid_integer_argument`
    * `tec_rest_invalid_number_argument`
    * `tec_rest_invalid_string_argument`
    * `tec_rest_invalid_date_time_argument`
    * `tec_rest_invalid_date_argument`
    * `tec_rest_invalid_uri_argument`
    * `tec_rest_invalid_email_argument`
  * The inner `data` is `{status:int, argument:string, details:string}`
    (`C:Exceptions/InvalidRestArgumentException.php:148-158`).
* `rest_forbidden` (401 when not logged in, 403 when logged in). Message:
  `"Sorry, you are not allowed to do that."`.
* `rest_no_route` (404), e.g. POST on `/{id}`, or a non-digit id.
* `rest_cookie_invalid_nonce` (403), `rest_missing_callback_param` (400) and other core codes.
* `missing_experimental_endpoint_acknowledgement` (400, `data:{status:400}`), §1.3.
* `InvalidRestArgumentException` thrown inside `filter_before_request` (`OpenAPI_Schema.php:301-310`)
  becomes `{"code":"tec_rest_invalid_query_argument"|"tec_rest_invalid_path_argument"|"tec_rest_invalid_body_argument"|"tec_rest_required_property_missing", "message":..., "data":{"status":400,"argument":..., "details":...}}`.
  This is only reachable for required params, of which there are none in TEC v1, so it is
  practically unreachable.

### 9.2 TEC ad-hoc `{"error": string}` shape (handler-level)

Body is exactly `{"error":"<message>"}`, with no `code` and no `data`. The HTTP status carries the
meaning.

| Status | Message | Where |
|---|---|---|
| 404 | `The entity could not be read.` | `Read_Entity_Response.php:43-57, 92-98` |
| 404 | `The entity could not be updated.` | `Update_Entity_Response.php:41-56` |
| 404 | `The entity could not be deleted.` | `Delete_Entity_Response.php:37-52` |
| 404 | `The entity could not be found.` | `Delete_Entity_Response.php:57-63` |
| 410 | `The entity has already been trashed.` | `:125-130` |
| 500 | `Failed to create entity.` | `Create_Entity_Response.php:41-47` |
| 500 | `Failed to update entity.` | `Update_Entity_Response.php:71-77` |
| 500 | `Entity not found after update.` | `:89-95` |
| 500 | `Force deletion failed - the entity could not be permanently deleted.` | `Delete_Entity_Response.php:71-77` |
| 500 | `The entity could not be permanently deleted.` | `:113-119` |
| 500 | `The entity could not be trashed.` | `:136-142` |
| 501 | `The entity does not support trashing. Set 'force=true' to delete.` | `:87-93` |

Messages are translatable (`tribe-common` text domain), so do not match on them.

### 9.3 Archive page-not-found `{code, message}` (no `data`)

See §8. It is HTTP 404 with code `tec_rest_organizers_page_not_found`.

Suggested Rust model:

```rust
#[serde(untagged)]
enum TecError {
    Wp { code: String, message: String, data: Option<WpErrorData> },   // 9.1 and 9.3 (data absent)
    Simple { error: String },                                          // 9.2
}
```

---

## 10. Differences from WordPress core `/wp/v2/posts`

1. **Namespace/paths:** `tec/v1/{events,venues,organizers}`. (The core `wp/v2/tribe_events` exists
   only for admins: `show_in_rest = current_user_can('manage_options')`, `src/Tribe/Main.php:1015`.)
2. **Mandatory `X-TEC-EEA` header** (§1.3).
3. **Response:**
   * Core fields as in §6.2, with `meta` and `_links` always removed.
   * `link` is replaced by the model permalink.
   * TEC fields are appended.
   * Events nest full Venue/Organizer objects.
   * There is no `_embed` support.
   * Password-protected posts are mangled much more aggressively than core (§6.7).
4. **Requests:**
   * `title`, `content` and `excerpt` are **plain strings**, not `{raw}` objects, and are run
     through `sanitize_text_field` (HTML stripped).
   * Core's `categories`/`tags` taxonomy fields are replaced by TEC-specific ones.
   * `featured_media`, `format` and `template` do not do what core does (stored as meta).
   * Defaults are injected on update (§7.3).
   * Unknown params are silently dropped.
   * Booleans use PHP `(bool)` casting (`"false"` is true).
5. **Collection params:** none of core's `context`, `offset`, `include`, `exclude`, `before`,
   `after`, `slug`, `author`, `orderby=id/relevance/...` are declared. They are dropped, except
   `context` (§6.1). `order` must be uppercase.
6. **Pagination:** same `X-WP-Total`/`X-WP-TotalPages`/`Link` headers as core.
   * Out-of-range pages give a 404 with a non-standard body and code. Core returns
     `rest_post_invalid_page_number` with 400.
   * `TotalPages` can be 0.
7. **Update** is `PUT|PATCH` only (no POST).
8. **Delete:**
   * Core returns the post (trash) or `{deleted:true, previous:{…}}` (force). TEC returns **`[]`**.
   * Core returns 410 `rest_already_trashed` and 501 `rest_trash_not_supported` as `WP_Error`s; TEC
     returns `{"error":…}` bodies with the same statuses.
9. **Nonexistent IDs** return 401/403 (not core's `404 rest_post_invalid_id`).
10. **Collection status filter** for non-editors is silently forced to `publish` (core returns a
    400/401 error).

---

## 11. Recommended Rust typing (summary)

* Model `Entity<T>` as the core fields plus `#[serde(flatten)] ext: T` plus
  `#[serde(flatten)] other: Map<String, Value>`.
  * Core fields required: `id: u64`, `date: String`, `date_gmt: Option<String>`, `guid: Rendered`,
    `modified`, `modified_gmt`, `slug`, `status: PostStatus`, `type: String`, `link: String`,
    `title: Rendered`.
  * Core fields optional: `content`/`excerpt: Option<RenderedProtected{rendered:String, protected:Option<bool>}>`,
    `author: Option<u64>`, `featured_media: Option<u64>`, `template: Option<String>`,
    `tags`/`tribe_events_cat: Option<Vec<u64>>`, `class_list: Option<Vec<String>>`.
* Datetimes:
  * Core (`date`, `modified`…): `YYYY-MM-DDTHH:MM:SS` without offset. Use
    `chrono::NaiveDateTime` with an optional offset, or keep as `String`.
  * TEC (`start_date`, …): `YYYY-MM-DD HH:MM:SS`, as `NaiveDateTime` in `timezone`.
  * `Date.date`: `YYYY-MM-DD HH:MM:SS.ffffff`.
* Event:
  * `duration`: string-or-int.
  * `multiday`: `false`-or-int.
  * `*_this_week`: `Option`.
  * `displays_on: Vec<String>`.
  * `organizers: Vec<Entity<OrganizerExt>>`, `venues: Vec<Entity<VenueExt>>`.
  * `ticketed: Option<TicketedOrFalse>`.
* Password mangling: every non-whitelisted string may be `"Password protected"`, every
  bool/nullable may be `null`, and `Date.timezone_type` may be `0`. Either check
  `content.protected` / `class_list` for `"post-password-required"` before strict parsing, or make
  the TEC fields lenient.
* Query encoding:
  * Booleans: always serialize as `1`/`0`, never `true`/`false`.
  * Arrays: serialize as `key[]=a&key[]=b` (or comma-join with at least 2 items).
* Request body: serialize with `skip_serializing_if = Option::is_none`. To avoid the update default
  injection, explicitly send `sticky`, `comment_status`, `ping_status` (and `format`) with the
  desired current values.
* `order` enum: `"ASC"`/`"DESC"` uppercase. `orderby` enum: `date|event_date|title|menu_order|modified`.
* Status enum (request): `publish|pending|draft|future|private`.

---

## 12. Consolidated quirk list

1. The `X-TEC-EEA` header is required on all entity endpoints. Its check runs after validation and
   permission checks.
2. Boolean params use `(bool)$value`: the query-string `"false"` is TRUE. `DELETE ?force=false`
   permanently deletes.
3. Nonexistent or wrong-type IDs return 401/403 `rest_forbidden`, not 404.
4. Collection filters `categories`, `tags`, `ticketed` (events) and `only_with_upcoming`
   (venues/organizers) are no-ops. `has_events` filters on any value.
5. A single-value `categories=5` / `tags=5` query is rejected (needs `[]` syntax or a comma).
6. `per_page` max 100 is documented but not enforced.
7. `order` must be uppercase. Enums are strict and case-sensitive.
8. `start_date`/`end_date` filters are inclusive (on-or-after / on-or-before).
9. Response `duration` is a numeric string. `multiday` is `false|int`.
10. `starts_this_week`, `ends_this_week`, `happens_this_week`, `this_week_duration` are always null,
    and `displays_on` is always `[]`, in REST.
11. `directions_link` contains `&#038;`. Website/email fields can be `""` despite `format: uri/email`.
12. `class_list`, `tags`, `tribe_events_cat` appear only on events. `type` is present but
    undocumented.
13. Password-protected entities are mangled (`"Password protected"`, `0`, `null`, epoch dates with
    `timezone_type: 0`).
14. Out-of-range page: 404 with code `tec_rest_organizers_page_not_found` (sic), no `data`.
    `X-WP-TotalPages` is 0 for empty results.
15. Delete success body is `[]`. Errors in handlers are `{"error": "..."}` with no code.
16. Request `content`, `title` and `excerpt` are HTML-stripped (`sanitize_text_field`).
17. Update injects `comment_status=open`, `ping_status=open`, `format=standard`, `sticky=false` when
    omitted (events get unstuck).
18. `featured_media`, `format` and `template` in request bodies are written as plain post meta and
    have no visible effect. `tags` on venues/organizers likewise.
19. `all_day: false` on update does not clear all-day.
20. Updating a trashed post returns 500. Reading a trashed post returns 401/403 (guest) or 404
    `{"error"}` (editor).
21. A body `id` overrides the path `{id}`.
22. Undeclared params are silently dropped. Non-editors' `status` filter is silently forced to
    `publish`.
23. Event `organizers`/`venues` silently omit linked posts the caller cannot read, while
    `organizer_names` still lists all of them.
24. `POST` on `/{id}` is not supported (`rest_no_route`). Use `PUT`/`PATCH`.
