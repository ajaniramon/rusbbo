{% set title = locale.hk_baneos %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <div class="seg">
    <a href="{{ hk.path }}/bans" class="{% if view == 'active' %}on{% endif %}">{{ locale.hk_activos|raw }}{{ activeCount }})</a>
    <a href="{{ hk.path }}/bans?view=history" class="{% if view == 'history' %}on{% endif %}">{{ locale.hk_historial|raw }}</a>
  </div>
</div>

<div class="card danger-zone mb">
  <div class="card-head"><h2>{{ locale.hk_banear_a_un_usuario|raw }}</h2></div>
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/users/action" data-confirm="{{ locale.hk_banear_a_este_usuario_se_le_desconectara_del_hot|raw }}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="do" value="ban"><input type="hidden" name="return" value="{{ hk.path }}/bans">
      <div class="row">
        <div class="field"><label>{{ locale.hk_usuario|raw }}</label><input type="text" name="name" required></div>
        <div class="field" style="grid-column: span 2"><label>{{ locale.hk_motivo_lo_vera_el_usuario|raw }}</label><input type="text" name="reason" maxlength="200" required></div>
        <div class="field"><label>{{ locale.hk_duracion|raw }}</label>
          <select name="hours"><option value="1">{{ locale.hk_1_hora|raw }}</option><option value="24" selected>{{ locale.hk_1_dia|raw }}</option><option value="72">{{ locale.hk_3_dias|raw }}</option><option value="168">{{ locale.hk_1_semana|raw }}</option><option value="720">{{ locale.hk_30_dias|raw }}</option><option value="0">{{ locale.hk_permanente|raw }}</option></select>
        </div>
      </div>
      <div class="form-actions"><label class="check"><input type="checkbox" name="machine" value="1"> {{ locale.hk_banear_tambien_su_equipo_machine_id|raw }}</label><span class="spacer" style="flex:1"></span><button class="btn danger" type="submit">{{ locale.hk_banear|raw }}</button></div>
    </form>
  </div>
</div>

<div class="card flush">
  <div class="card-body scroll">
    {% if bans is empty %}
      <div class="empty">{% if view == 'active' %}{{ locale.hk_no_hay_nadie_baneado_ahora_mismo|raw }}{% else %}{{ locale.hk_no_hay_baneos_antiguos|raw }}{% endif %}</div>
    {% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_quien|raw }}</th><th>{{ locale.hk_tipo|raw }}</th><th>{{ locale.hk_motivo|raw }}</th><th>{{ locale.hk_desde|raw }}</th><th>{{ locale.hk_hasta|raw }}</th><th>{{ locale.hk_por_2|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for b in bans %}
        <tr>
          <td>
            {% if b.ban_type == 'USER_ID' and b.username != '' %}
              <div class="user-cell"><img class="avatar-head" src="/habbo-imaging/avatarimage?figure={{ b.figure }}&size=s&head=1&direction=3&head_direction=3" alt="">{% if hk.can('user/search') %}<a href="{{ hk.path }}/users/view?id={{ b.user_id }}&tab=moderation">{{ b.username }}</a>{% else %}{{ b.username }}{% endif %}</div>
            {% elseif b.ban_type == 'MACHINE_ID' %}
              <span class="mono small">{{ b.banned_value }}</span><div class="muted small">{{ b.machine_users | default(locale.hk_sin_cuentas) }}</div>
            {% else %}
              <span class="mono">{{ b.banned_value }}</span>
            {% endif %}
          </td>
          <td>{% if b.ban_type == 'MACHINE_ID' %}{{ locale.hk_equipo|raw }}{% elseif b.ban_type == 'IP_ADDRESS' %}{{ locale.hk_ip|raw }}{% else %}{{ locale.hk_cuenta|raw }}{% endif %}</td>
          <td class="wrap">{{ b.message }}</td>
          <td class="nowrap muted">{{ b.banned_at }}</td>
          <td class="nowrap">{{ b.banned_until }}</td>
          <td>{{ b.banned_by }}</td>
          <td class="actions">
            {% if b.is_active %}
            <form class="inline" method="post" action="{{ hk.path }}/bans/unban" data-confirm="{{ locale.hk_retirar_este_baneo|raw }}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}">
              <input type="hidden" name="type" value="{{ b.ban_type }}">
              <input type="hidden" name="value" value="{{ b.banned_value }}">
              <button class="btn sm" type="submit">{{ locale.hk_desbanear|raw }}</button>
            </form>
            {% else %}<span class="pill">{{ locale.hk_terminado|raw }}</span>{% endif %}
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
  {% include "housekeeping/base/pager.tpl" %}
</div>
<p class="help mt">{{ locale.hk_no_se_ofrece_banear_por_ip_en_este_hotel_todos_e|raw }}</p>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
