{% set title = locale.hk_grupo ~ group.name() %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{{ locale.hk_grupo_2|raw }}{{ group.id() }} {{ locale.hk_creado|raw }} {{ group.createdAt() }} · {{ group.memberCount() }} {{ locale.hk_miembros|raw }} {{ group.threadCount() }} {{ locale.hk_temas|raw }}</p></div>
  <div class="actions"><a class="btn" href="{{ hk.path }}/groups">{{ locale.hk_volver_a_grupos|raw }}</a></div>
</div>

<div class="card">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/groups/edit?id={{ group.id() }}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row">
        <div class="field" style="grid-column: span 2"><label>{{ locale.hk_nombre|raw }}</label><input type="text" name="name" required value="{{ group.name() }}"></div>
        <div class="field"><label>{{ locale.hk_alias_url|raw }}</label><input type="text" name="alias" value="{{ group.alias() }}"></div>
        <div class="field"><label>{{ locale.hk_id_del_dueno|raw }}</label><input type="number" name="owner_id" value="{{ group.ownerId() }}"></div>
        <div class="field"><label>{{ locale.hk_id_de_su_sala|raw }}</label><input type="number" name="room_id" value="{{ group.roomId() }}"></div>
      </div>
      <div class="row">
        <div class="field"><label>{{ locale.hk_placa|raw }}</label><input type="text" name="badge" class="mono" value="{{ group.badge() }}"></div>
        <div class="field"><label>{{ locale.hk_fondo|raw }}</label><input type="text" name="background" value="{{ group.background() }}"></div>
        <div class="field"><label>{{ locale.hk_visitas|raw }}</label><input type="number" name="views" value="{{ group.views() }}"></div>
        <div class="field"><label>{{ locale.hk_temas_2|raw }}</label><input type="number" name="topics" value="{{ group.topics() }}"></div>
        <div class="field"><label>{{ locale.hk_tipo_de_grupo|raw }}</label><input type="number" name="group_type" value="{{ group.groupType() }}"><span class="help">{{ locale.hk_0_normal_1_exclusivo_2_cerrado|raw }}</span></div>
      </div>
      <div class="row r-3">
        <div class="field"><label>{{ locale.hk_foro|raw }}</label><select name="forum_type"><option value="0" {% if group.forumType() == 0 %}selected{% endif %}>{{ locale.hk_publico|raw }}</option><option value="1" {% if group.forumType() == 1 %}selected{% endif %}>{{ locale.hk_privado|raw }}</option></select></div>
        <div class="field"><label>{{ locale.hk_quien_puede_escribir|raw }}</label><select name="forum_premission"><option value="0" {% if group.forumPermission() == 0 %}selected{% endif %}>{{ locale.hk_todos|raw }}</option><option value="1" {% if group.forumPermission() == 1 %}selected{% endif %}>{{ locale.hk_miembros_2|raw }}</option><option value="2" {% if group.forumPermission() == 2 %}selected{% endif %}>{{ locale.hk_administradores|raw }}</option></select></div>
        <div class="field" style="justify-content:flex-end"><label class="check"><input type="checkbox" name="recommended" {% if group.recommended() %}checked{% endif %}> {{ locale.hk_recomendado|raw }}</label></div>
      </div>
      <div class="field"><label>{{ locale.hk_descripcion|raw }}</label><textarea name="description" rows="3">{{ group.description() }}</textarea></div>
      <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_guardar_grupo|raw }}</button></div>
    </form>
  </div>
</div>

<div class="card flush mt">
  <div class="card-head"><h2>{{ locale.hk_miembros_2|raw }} <span class="sub">{{ members | length }}</span></h2></div>
  <div class="card-body scroll">
    {% if members is empty %}<div class="empty">{{ locale.hk_sin_miembros|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_usuario|raw }}</th><th>{{ locale.hk_rango|raw }}</th><th>{{ locale.hk_pendiente|raw }}</th><th>{{ locale.hk_desde|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for m in members %}
        <tr>
          <td><a href="{{ hk.path }}/users/view?id={{ m.userId() }}">{{ m.username() }}</a></td>
          <td>
            <select name="member_rank" form="gm-{{ m.userId() }}">
              <option value="1" {% if m.memberRank() == 1 %}selected{% endif %}>{{ locale.hk_miembro|raw }}</option>
              <option value="2" {% if m.memberRank() == 2 %}selected{% endif %}>{{ locale.hk_administrador|raw }}</option>
              <option value="3" {% if m.memberRank() == 3 %}selected{% endif %}>{{ locale.hk_dueno|raw }}</option>
            </select>
          </td>
          <td><input type="checkbox" name="is_pending" form="gm-{{ m.userId() }}" {% if m.pending() %}checked{% endif %}></td>
          <td class="nowrap muted">{{ m.createdAt() }}</td>
          <td class="actions">
            <form class="inline" id="gm-{{ m.userId() }}" method="post" action="{{ hk.path }}/groups/member">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="group_id" value="{{ group.id() }}"><input type="hidden" name="user_id" value="{{ m.userId() }}">
              <button class="btn sm" type="submit">{{ locale.hk_guardar|raw }}</button>
            </form>
            <form class="inline" method="post" action="{{ hk.path }}/groups/member?group_id={{ group.id() }}&user_id={{ m.userId() }}&delete=true" data-confirm="{{ locale.hk_expulsar_a|raw }} {{ m.username() }} {{ locale.hk_del_grupo|raw }}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm ghost-danger" type="submit">{{ locale.hk_expulsar|raw }}</button>
            </form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
</div>

<div class="card flush mt">
  <div class="card-head"><h2>{{ locale.hk_temas_del_foro|raw }} <span class="sub">{{ threads | length }}</span></h2></div>
  <div class="card-body scroll">
    {% if threads is empty %}<div class="empty">{{ locale.hk_sin_temas|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_titulo|raw }}</th><th>{{ locale.hk_autor|raw }}</th><th class="num">{{ locale.hk_respuestas|raw }}</th><th>{{ locale.hk_estado|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for t in threads %}
        <tr>
          <td><input type="text" name="topic_title" form="gt-{{ t.id() }}" value="{{ t.topicTitle() }}"></td>
          <td>{{ t.posterName() }}</td>
          <td class="num">{{ t.replyCount() }}</td>
          <td class="nowrap">
            <label class="check"><input type="checkbox" name="is_open" form="gt-{{ t.id() }}" {% if t.open() %}checked{% endif %}> {{ locale.hk_abierto|raw }}</label>
            <label class="check"><input type="checkbox" name="is_stickied" form="gt-{{ t.id() }}" {% if t.stickied() %}checked{% endif %}> {{ locale.hk_fijado|raw }}</label>
          </td>
          <td class="actions">
            <form class="inline" id="gt-{{ t.id() }}" method="post" action="{{ hk.path }}/groups/thread">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="group_id" value="{{ group.id() }}"><input type="hidden" name="thread_id" value="{{ t.id() }}">
              <button class="btn sm" type="submit">{{ locale.hk_guardar|raw }}</button>
            </form>
            <form class="inline" method="post" action="{{ hk.path }}/groups/thread?group_id={{ group.id() }}&thread_id={{ t.id() }}&delete=true" data-confirm="{{ locale.hk_borrar_el_tema_con_todas_sus_respuestas|raw }}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm ghost-danger" type="submit">{{ locale.hk_borrar|raw }}</button>
            </form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
</div>

<div class="card flush mt">
  <div class="card-head"><h2>{{ locale.hk_respuestas_recientes|raw }}</h2></div>
  <div class="card-body scroll">
    {% if replies is empty %}<div class="empty">{{ locale.hk_sin_respuestas|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_tema|raw }}</th><th>{{ locale.hk_autor|raw }}</th><th>{{ locale.hk_mensaje|raw }}</th><th>{{ locale.hk_estado|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for r in replies %}
        <tr>
          <td>{{ r.topicTitle() }}</td>
          <td>{{ r.posterName() }}</td>
          <td class="wrap">{{ r.message() }}</td>
          <td>{% if r.deleted() %}<span class="pill warn">{{ locale.hk_oculta|raw }}</span>{% else %}<span class="pill ok">{{ locale.hk_visible|raw }}</span>{% endif %}{% if r.edited() %} <span class="pill">{{ locale.hk_editada|raw }}</span>{% endif %}</td>
          <td class="actions">
            <form class="inline" method="post" action="{{ hk.path }}/groups/reply?group_id={{ group.id() }}&reply_id={{ r.id() }}&deleted={% if r.deleted() %}false{% else %}true{% endif %}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm" type="submit">{% if r.deleted() %}{{ locale.hk_restaurar|raw }}{% else %}{{ locale.hk_ocultar|raw }}{% endif %}</button>
            </form>
            <form class="inline" method="post" action="{{ hk.path }}/groups/reply?group_id={{ group.id() }}&reply_id={{ r.id() }}&delete=true" data-confirm="{{ locale.hk_borrar_esta_respuesta_para_siempre|raw }}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm ghost-danger" type="submit">{{ locale.hk_borrar|raw }}</button>
            </form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
</div>

<div class="card danger-zone mt">
  <div class="card-head"><h2>{{ locale.hk_zona_peligrosa|raw }}</h2></div>
  <div class="card-body">
    <form class="inline" method="post" action="{{ hk.path }}/groups/delete?id={{ group.id() }}" data-confirm="{{ locale.hk_borrar_el_grupo|raw }}{{ group.name() }}{{ locale.hk_no_se_puede_deshacer_2|raw }}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn danger" type="submit">{{ locale.hk_borrar_grupo|raw }}</button>
    </form>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
