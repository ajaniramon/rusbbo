{% set title = locale.hk_infobus %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="grid g-side mb">
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_control_del_infobus|raw }}</h2></div>
    <div class="card-body form-actions">
      <form class="inline" method="post" action="{{ hk.path }}/infobus_polls/door_status?status=1"><input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn" type="submit">{{ locale.hk_abrir_puertas|raw }}</button></form>
      <form class="inline" method="post" action="{{ hk.path }}/infobus_polls/door_status?status=0"><input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn" type="submit">{{ locale.hk_cerrar_puertas|raw }}</button></form>
      <form class="inline" method="post" action="{{ hk.path }}/infobus_polls/close_event" data-confirm="{{ locale.hk_terminar_el_evento_del_infobus_saldran_todos_del|raw }}"><input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn ghost-danger" type="submit">{{ locale.hk_terminar_evento|raw }}</button></form>
    </div>
  </div>
  <div class="card">
    <div class="card-body">
      <p class="muted" style="margin:0 0 10px">{{ locale.hk_el_infobus_es_la_sala_publica_de_encuestas_abre|raw }}</p>
      <a class="btn primary" href="{{ hk.path }}/infobus_polls/create">{{ locale.hk_nueva_encuesta|raw }}</a>
    </div>
  </div>
</div>

<div class="card flush">
  <div class="card-body scroll">
    {% if infobusPolls is empty %}<div class="empty">{{ locale.hk_todavia_no_hay_encuestas|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_pregunta|raw }}</th><th>{{ locale.hk_respuestas_posibles|raw }}</th><th>{{ locale.hk_creada_por|raw }}</th><th>{{ locale.hk_fecha|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for p in infobusPolls %}
        <tr>
          <td><strong>{{ p.getPollData().getQuestion() }}</strong></td>
          <td class="small muted">{% for a in p.getPollData().getAnswers() %}{{ a }}{% if not loop.last %} · {% endif %}{% endfor %}</td>
          <td>{{ p.getCreator() }}</td>
          <td class="nowrap muted">{{ p.getCreatedAtFormatted() }}</td>
          <td class="actions">
            <form class="inline" method="post" action="{{ hk.path }}/infobus_polls/send_poll?id={{ p.id }}" data-confirm="{{ locale.hk_lanzar_esta_encuesta_a_los_que_estan_en_el_infob|raw }}"><input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm primary" type="submit">{{ locale.hk_lanzar|raw }}</button></form>
            <a class="btn sm" href="{{ hk.path }}/infobus_polls/view_results?id={{ p.id }}">{{ locale.hk_resultados|raw }}</a>
            <a class="btn sm" href="{{ hk.path }}/infobus_polls/edit?id={{ p.id }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/infobus_polls/clear_results?id={{ p.id }}" data-confirm="{{ locale.hk_borrar_todas_las_respuestas_de_esta_encuesta|raw }}"><input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm" type="submit">{{ locale.hk_vaciar_respuestas|raw }}</button></form>
            <form class="inline" method="post" action="{{ hk.path }}/infobus_polls/delete?id={{ p.id }}" data-confirm="{{ locale.hk_borrar_la_encuesta|raw }}"><input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm ghost-danger" type="submit">{{ locale.hk_borrar|raw }}</button></form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
