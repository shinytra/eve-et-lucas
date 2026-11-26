## Default Permission

Permissions par défaut du géofencing : définir ou effacer la maison, lire la
position actuelle, choisir si ce téléphone sonne à l'arrivée, vérifier et
demander les autorisations Android.

La lecture des arrivées enregistrées (`peek_arrivals`, `ack_arrivals`) et le
réveil par ntfy (`set_push_config`, `register_push`, `peek_received`,
`ack_received`, `mark_rung`) restent réservés au code Rust de l'appli : ils ne
sont pas exposés à l'interface (la clé de chiffrement ne passe jamais par elle).

#### This default permission set includes the following:

- `allow-set-home`
- `allow-clear-home`
- `allow-current-position`
- `allow-set-ring-on-arrival`
- `allow-check-permissions`
- `allow-request-permissions`

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`geofence:allow-check-permissions`

</td>
<td>

Enables the check_permissions command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`geofence:deny-check-permissions`

</td>
<td>

Denies the check_permissions command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`geofence:allow-clear-home`

</td>
<td>

Enables the clear_home command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`geofence:deny-clear-home`

</td>
<td>

Denies the clear_home command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`geofence:allow-current-position`

</td>
<td>

Enables the current_position command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`geofence:deny-current-position`

</td>
<td>

Denies the current_position command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`geofence:allow-request-permissions`

</td>
<td>

Enables the request_permissions command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`geofence:deny-request-permissions`

</td>
<td>

Denies the request_permissions command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`geofence:allow-set-home`

</td>
<td>

Enables the set_home command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`geofence:deny-set-home`

</td>
<td>

Denies the set_home command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`geofence:allow-set-ring-on-arrival`

</td>
<td>

Enables the set_ring_on_arrival command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`geofence:deny-set-ring-on-arrival`

</td>
<td>

Denies the set_ring_on_arrival command without any pre-configured scope.

</td>
</tr>
</table>
